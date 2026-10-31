# PTY I/O Pump Contract

## Ownership and execution model

PtyIoPump owns one RuntimeProcess handle and moves it to a dedicated worker thread. The caller never performs blocking process I/O. The worker uses only the RuntimeProcess non-blocking read/write, resize, signal, wait, and termination methods.

One pump represents one terminal session. Do not share a RuntimeProcess handle or pump between sessions. The process handle is dropped by the worker after normal exit or cancellation.

## Bounded queues and backpressure

| Resource | Bound |
| --- | ---: |
| Input chunk | 4096 bytes |
| Pending command queue | 16 commands |
| Output event queue | 8 events |
| Output event payload | 4096 bytes |
| Worker pending input | 1 chunk (at most 4096 bytes) |
| Worker pending output/control event | 1 event (at most 4096 bytes for output) |

The owner submits input with try_write. A Backpressure result means no bytes were accepted; the caller retains the input and retries later. A successful submission transfers an owned copy to the bounded command queue.

The worker reads at most one output chunk ahead of the bounded event queue. If the consumer is slow, the worker retains at most one pending output event and stops reading the PTY. Kernel PTY buffering then applies natural backpressure to the child. It never appends output to an unbounded application buffer.

Output events preserve byte order. Each Output payload is an owned byte vector no larger than 4096 bytes. UTF-8 and VT sequence boundaries may cross events; consumers must feed the terminal parser incrementally.

## Command ordering

The command queue is FIFO. Input chunks are written in submission order. A partially written chunk remains worker-owned until all its bytes are accepted; later commands are not reordered ahead of it.

Resize, signal, stdin-close, and termination commands are applied by the same worker that owns the process handle. A successful resize emits Resized only after the runtime reports that the kernel accepted the new dimensions. Runtime failures are emitted as Error events.

## Output and exit ordering

- Output is emitted in the exact byte order returned by the runtime.
- Eof is emitted once when the PTY output stream reaches EOF.
- Exited is emitted once, after Eof has been delivered and the runtime has supplied a final process status.
- A child exit does not discard bytes still buffered in the PTY; the worker drains output until EOF before emitting Exited.
- Runtime I/O failures emit Error and trigger cleanup after the error event is delivered.

## Cancellation and shutdown

shutdown sets an atomic cancellation flag and unparks the worker. It does not enqueue a shutdown command, so a full input queue cannot prevent cancellation.

The worker requests graceful termination, polls try_wait for a bounded 250 ms grace period, then requests forced termination and reaps the child. The worker thread is joined before shutdown returns. Drop invokes the same cancellation path and never panics.

If the event receiver is disconnected, the worker terminates and reaps the child rather than continuing to produce data no consumer can receive.

## Threading and blocking rules

- RuntimeProcess is moved to one worker thread.
- The owner thread uses bounded try_write/resize/signal/close/terminate submissions.
- try_recv_event is non-blocking; recv_event_timeout blocks only the calling thread for its specified timeout.
- No worker operation waits for the GUI thread.
- The worker parks briefly when non-blocking operations return WouldBlock and is unparked by submitted commands or event consumption.
- Cancellation is independent of the command queue and always wakes a parked worker.

## Failure behavior

- Input larger than 4096 bytes returns InputTooLarge before queueing.
- A full command queue returns Backpressure without accepting the command.
- A disconnected worker returns Closed.
- Runtime operation failures are surfaced as Error events.
- Invalid backend read/write progress (zero or more bytes than supplied/buffered) is treated as InvalidArgument and triggers cleanup.
- Worker thread creation and panic/join failures are reported as IoPumpError.
- A failed shutdown still attempts process termination and reaping before the worker exits.

## Verification

Unit tests use a deterministic fake RuntimeProcess to verify:
- high-volume output stops at the bounded event queue when the consumer is slow;
- draining events resumes output without loss or reordering;
- partial writes and a full command queue preserve exact input order;
- resize, EOF, and Exited event ordering;
- shutdown bypasses a full queue and terminates the process.

Linux integration tests use a real PTY child to verify kernel resize propagation, output EOF/exit delivery, and cancellation while a high-volume producer is blocked by a slow consumer.
