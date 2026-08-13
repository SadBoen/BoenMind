# 待提交上游 Issue 草稿（asupersync，2026-08-14）

> 提交目标：https://github.com/Dicklesworthstone/asupersync/issues/new
> 状态：草稿。提交后在本文件头部补 issue 链接，并从台账「待提」更新为 issue 号。
> 与 `UPSTREAM_PATCHES.md` 的 A1/A2 补丁对应。

---

## Title

**Windows: `getpeername()` reports success before connect completes — TLS handshake fails with WSAENOTCONN (10057) when embedding without an io/timer driver**

## Body

### Summary

When embedding asupersync from an external executor (e.g. `futures::executor::block_on` or tokio — as documented in the pi_agent_rust SDK cookbook), `Cx::current()` has no io driver, so TCP connect completion falls back to `wait_for_connect_fallback`, which polls `peer_addr()`/`take_error()`. On some Windows network stacks (reproduced on Windows 10.0.19044 with plain routes to Aliyun datacenter IPs), **`getpeername()` returns success before the TCP connect has actually completed**. The fallback then declares the connection ready, the TLS handshake's first write hits `WSAENOTCONN` (os error 10057), and the retry budget (`MAX_CONNECT_SETTLE_RETRIES = 4096` busy-wakes, no timer driver → immediate re-wake) is exhausted in microseconds — long before the connect settles (~40 ms). The handshake fails.

Observed symptom in pi_agent_rust SDK consumers: intermittent `TLS connect failed: ... os error 10057`; the failure rate depends on which resolved IP is connected to (fast/slow connect), which made it look like flaky networking.

### Minimal repro (socket2, Windows)

```rust
let sock = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
sock.set_nonblocking(true)?;
match sock.connect(&addr.into()) {
    Err(e) if e.kind() == ErrorKind::WouldBlock => {
        // poll #0 (immediately):
        sock.peer_addr()   // -> Ok(...)  !!! connect not complete yet
        sock.send(&[0x16]) // -> WSAENOTCONN (10057)
        // poll #1 (~10ms later):
        sock.send(&[0x16]) // -> Ok(1)  connect has settled
    }
    ...
}
```

`peer_addr()` returning `Ok` while the socket is still connecting is not standard Windows behavior per MSDN (expected `WSAENOTCONN`), but it occurs on this machine's stack (Win10 19044, plain Aliyun routes; no third-party LSP, no VPN, no proxy — system stack `std::net` and curl are unaffected). Regardless of which side is "at fault", the fallback's completion detection is the fragile part: it relies on a query that can lie on some stacks.

### Suggested fixes

1. **`wait_for_connect_fallback()` — use kernel readiness, not `peer_addr()`** (primary fix).
   On Windows, poll the socket with `WSAPoll` (or the already-available `polling` crate) for `WRITABLE`; writable (or error) is the real connect-completion signal. No timing threshold, no misjudgment. A local patch implementing this resolves the issue 100% (10/10 runs, previously ~50% failure).

2. **`poll_write()` — add a real-time floor to the 10057 retry** (defense-in-depth).
   The retry currently burns `MAX_CONNECT_SETTLE_RETRIES` in microseconds when no timer driver exists (busy re-wake). Enforcing at least ~100 ms of wall time before giving up lets a genuinely settling connection survive (observed settle time ~40 ms).

### Notes

- Upstream CLI runs inside an asupersync runtime where the io driver exists, so this path is only hit by **library/embedded consumers** (e.g. pi_agent_rust's documented `futures::block_on` SDK usage) — which is why it wasn't caught by the CLI's own tests.
- `asupersync 0.3.10` already contains `#35`/`#106`-related retry logic for 10057; this report is about the remaining gap: (a) completion detection via `getpeername` and (b) retry budget without a real-time floor in the driver-less fallback path.

### Environment

- OS: Windows 10.0.19044 x64
- Target: api.minimaxi.com (5 Aliyun IPs), api.deepseek.com — all fail identically; curl/system stack fine
- asupersync 0.3.10, socket2 0.5
