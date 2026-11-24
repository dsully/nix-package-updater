//! OSC 7501 Program Status Protocol: <https://www.superlogical.com/rex/docs/build/program-status>

use std::io::{IsTerminal, Write};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

#[derive(Clone, Copy)]
pub enum State {
    Working,
    Done,
    Error,
}

fn encode(state: State, msg: &str) -> String {
    let state = match state {
        State::Working => "working",
        State::Done => "done",
        State::Error => "error",
    };

    format!("\x1b]7501;state={state}:app=nix:msg={}\x1b\\", STANDARD.encode(msg))
}

pub fn report(state: State, msg: &str) {
    let mut stderr = std::io::stderr();

    if stderr.is_terminal() {
        let _ = stderr.write_all(encode(state, msg).as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_report() {
        assert_eq!(encode(State::Working, "Building"), "\x1b]7501;state=working:app=nix:msg=QnVpbGRpbmc=\x1b\\");
    }
}
