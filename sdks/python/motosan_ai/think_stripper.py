from __future__ import annotations


class ThinkStripper:
    """Stateful streamer that buffers and removes <think>...</think> across chunks."""

    def __init__(self) -> None:
        self._buf = ""
        self._in_think = False

    def flush(self) -> str:
        """Flush any remaining buffered text. Call when the stream ends."""
        remaining = self._buf
        self._buf = ""
        if self._in_think:
            self._in_think = False
            return ""
        return remaining

    def feed(self, chunk: str) -> str:
        """Returns clean text to emit; may return empty string while buffering."""
        self._buf += chunk
        output: list[str] = []

        while self._buf:
            if self._in_think:
                end = self._buf.find("</think>")
                if end == -1:
                    # Keep just enough to detect a split </think>
                    keep = len("</think>") - 1
                    if len(self._buf) > keep:
                        self._buf = self._buf[-keep:]
                    break
                self._buf = self._buf[end + len("</think>"):]
                self._in_think = False
            else:
                start = self._buf.find("<think>")
                if start == -1:
                    # Safe to emit, but hold tail in case tag is split
                    safe = len(self._buf) - (len("<think>") - 1)
                    if safe > 0:
                        output.append(self._buf[:safe])
                        self._buf = self._buf[safe:]
                    break
                output.append(self._buf[:start])
                self._buf = self._buf[start + len("<think>"):]
                self._in_think = True

        return "".join(output)
