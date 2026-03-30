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
                        if self.buf.len() > keep {
                            self.buf = self.buf[self.buf.len() - keep..].to_string();
                        }
                        break;
                    }
                    Some(end) => {
                        self.buf = self.buf[end + "</think>".len()..].to_string();
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
                        output.push_str(&self.buf[..safe]);
                        self.buf = self.buf[safe..].to_string();
                        break;
                    }
                    Some(start) => {
                        output.push_str(&self.buf[..start]);
                        self.buf = self.buf[start + "<think>".len()..].to_string();
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
}
