//! Running the OneDrive client with its standard output and error merged into one stream, in
//! the order it wrote them (Python's `stderr=subprocess.STDOUT`), without a shell in between.

use std::ffi::OsStr;
use std::io::{PipeReader, Read};
use std::process::{Child, Command, Stdio};

/// Start `program` with both output streams on one pipe. Standard input is closed, so a client
/// that stops to ask a question gets end-of-file instead of waiting forever.
pub fn spawn_merged<S: AsRef<OsStr>>(program: &str, args: &[S]) -> std::io::Result<(Child, PipeReader)> {
    let (reader, writer) = std::io::pipe()?;
    // The `Command` (holding our copies of the write end) is dropped at the end of this
    // statement, so the reader sees end-of-file once the client exits.
    let child = Command::new(program).args(args).stdin(Stdio::null()).stdout(writer.try_clone()?).stderr(writer).spawn()?;
    Ok((child, reader))
}

/// Longest line kept; the rest of an overlong line is dropped (a runaway line can't eat memory).
const MAX_LINE: usize = 64 * 1024;

/// Call `on_line` for every line `reader` produces until end-of-file. `\n`, `\r\n` and a bare
/// `\r` (progress updates) all end a line; invalid UTF-8 is replaced.
pub fn for_each_line(mut reader: impl Read, mut on_line: impl FnMut(&str)) {
    let mut line: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                log::warn!("[GUI] reading client output: {e}");
                break;
            }
        };
        for &byte in chunk.get(..n).unwrap_or_default() {
            if byte == b'\n' || byte == b'\r' {
                if !line.is_empty() {
                    on_line(&String::from_utf8_lossy(&line));
                    line.clear();
                }
            } else if line.len() < MAX_LINE {
                line.push(byte);
            }
        }
    }
    if !line.is_empty() {
        on_line(&String::from_utf8_lossy(&line));
    }
}

/// Run `program` to completion, returning every output line (both streams, merged).
pub fn run_lines<S: AsRef<OsStr>>(program: &str, args: &[S]) -> std::io::Result<(Vec<String>, std::process::ExitStatus)> {
    let (mut child, reader) = spawn_merged(program, args)?;
    let mut lines = Vec::new();
    for_each_line(reader, |l| lines.push(l.to_string()));
    let status = child.wait()?;
    Ok((lines, status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carriage_returns_end_lines() {
        let mut lines = Vec::new();
        for_each_line(&b"one\r\ntwo\rthree\n\n\xffbad\nlast"[..], |l| lines.push(l.to_string()));
        assert_eq!(lines, ["one", "two", "three", "\u{fffd}bad", "last"]);
    }

    #[test]
    fn both_streams_are_captured() {
        let (lines, status) = run_lines("sh", &["-c", "echo out; echo err >&2; exit 3"]).unwrap();
        assert_eq!(lines, ["out", "err"]);
        assert_eq!(status.code(), Some(3));
        assert!(run_lines("/nonexistent/onedrive", &["--version"]).is_err());
    }
}
