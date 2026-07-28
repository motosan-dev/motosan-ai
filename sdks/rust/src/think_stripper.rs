/// Stateful filter that buffers and removes `<think>...</think>` tags
/// across streaming chunks.
#[derive(Default)]
pub struct ThinkStripper {
    buf: String,
    in_think: bool,
}

impl ThinkStripper {
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            in_think: false,
        }
    }

    /// Flush any remaining buffered text. Call when the stream ends.
    pub fn flush(&mut self) -> String {
        let remaining = std::mem::take(&mut self.buf);
        if self.in_think {
            self.in_think = false;
            String::new()
        } else {
            remaining
        }
    }

    pub fn feed(&mut self, chunk: &str) -> String {
        self.buf.push_str(chunk);
        let mut output = String::new();
        loop {
            if self.in_think {
                match self.buf.find("</think>") {
                    None => {
                        let keep = "</think>".len() - 1;
                        let mut cut = self.buf.len().saturating_sub(keep);
                        // Ensure we split on a char boundary (important for multi-byte UTF-8)
                        while cut > 0 && !self.buf.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        self.buf.drain(..cut);
                        break;
                    }
                    Some(end) => {
                        self.buf.drain(..end + "</think>".len());
                        self.in_think = false;
                    }
                }
            } else {
                match self.buf.find("<think>") {
                    None => {
                        let keep = "<think>".len() - 1;
                        let mut safe = self.buf.len().saturating_sub(keep);
                        // Ensure we split on a char boundary (important for multi-byte UTF-8)
                        while safe > 0 && !self.buf.is_char_boundary(safe) {
                            safe -= 1;
                        }
                        if output.is_empty() {
                            // Fast path: hand the buffer's allocation to the caller;
                            // self.buf becomes the freshly-split tiny tail. This removes
                            // the second full-size copy AND the full-size tail realloc
                            // (the tail alloc is ≤ a few bytes).
                            let tail = self.buf.split_off(safe);
                            return std::mem::replace(&mut self.buf, tail);
                        }
                        output.push_str(&self.buf[..safe]);
                        self.buf.drain(..safe);
                        break;
                    }
                    Some(start) => {
                        output.push_str(&self.buf[..start]);
                        self.buf.drain(..start + "<think>".len());
                        self.in_think = true;
                    }
                }
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_complete_think_block() {
        let mut s = ThinkStripper::new();
        let mut out = s.feed("<think>reasoning</think>answer is here");
        out.push_str(&s.flush());
        assert_eq!(out, "answer is here");
    }

    #[test]
    fn strips_think_block_across_chunks() {
        let mut s = ThinkStripper::new();
        assert_eq!(s.feed("<think>reas"), "");
        let mut out = s.feed("oning</think>answer is here");
        out.push_str(&s.flush());
        assert_eq!(out, "answer is here");
    }

    #[test]
    fn strips_split_open_tag() {
        let mut s = ThinkStripper::new();
        let mut out = String::new();
        out.push_str(&s.feed("hello<thi"));
        out.push_str(&s.feed("nk>hidden</think>world!"));
        out.push_str(&s.flush());
        assert_eq!(out, "helloworld!");
    }

    #[test]
    fn strips_split_close_tag() {
        let mut s = ThinkStripper::new();
        assert_eq!(s.feed("<think>hidden</thi"), "");
        let mut out = s.feed("nk>visible text");
        out.push_str(&s.flush());
        assert_eq!(out, "visible text");
    }

    #[test]
    fn passes_through_without_tags() {
        let mut s = ThinkStripper::new();
        let mut out = String::new();
        out.push_str(&s.feed("hello "));
        out.push_str(&s.feed("world"));
        out.push_str(&s.flush());
        assert_eq!(out, "hello world");
    }

    #[test]
    fn strips_multiple_think_blocks() {
        let mut s = ThinkStripper::new();
        let mut out = s.feed("<think>a</think>between<think>c</think>after!");
        out.push_str(&s.flush());
        assert_eq!(out, "betweenafter!");
    }

    #[test]
    fn handles_empty_input() {
        let mut s = ThinkStripper::new();
        assert_eq!(s.feed(""), "");
        assert_eq!(s.flush(), "");
    }

    #[test]
    fn flush_while_in_think_returns_empty() {
        let mut s = ThinkStripper::new();
        s.feed("<think>still thinking");
        assert_eq!(s.flush(), "");
    }

    #[test]
    fn multibyte_thinking_content_does_not_panic() {
        let mut s = ThinkStripper::new();
        // 8 three-byte chars = 24 bytes; len - 7 = 17 is NOT a char boundary.
        assert_eq!(s.feed("<think>中文思考中文思考"), "");
        let mut out = s.feed("</think>答案");
        out.push_str(&s.flush());
        assert_eq!(out, "答案");
    }

    #[test]
    fn multibyte_passthrough_across_chunks() {
        let mut s = ThinkStripper::new();
        let mut out = String::new();
        out.push_str(&s.feed("回答是："));
        out.push_str(&s.feed("四十二。"));
        out.push_str(&s.flush());
        assert_eq!(out, "回答是：四十二。");
    }
}
