//! `read-chat` の視聴者一覧の周期取得を、取得のたびに `show-chatters` と同じ書式で追記するログ
//! （`[monitor] chatters_log`）。

use anyhow::Context;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// `show-chatters` と `chatters_log` の 1 行（改行なし）。配信者と bot 自身を除いた login を昇順に並べる。
pub(crate) fn format_chatters_line<'a>(
    now: chrono::NaiveDateTime,
    logins: impl IntoIterator<Item = &'a str>,
    channel_name: &str,
    username: &str,
) -> String {
    let mut users: Vec<&str> = logins
        .into_iter()
        .filter(|name| *name != channel_name && *name != username)
        .collect();
    users.sort_unstable();
    format!("{},{}", now.format("%Y-%m-%d %H:%M:%S"), users.join(","))
}

/// 視聴者一覧の追記先。行は `show-chatters` と同じく、配信者と bot 自身を除いた login 名。
pub struct ChattersLog {
    path: PathBuf,
    channel: String,
    username: String,
}

impl ChattersLog {
    /// 追記モードで開けることを確かめる（無ければ作る）。開けなければ OS のエラーとパスを返す。
    ///
    /// 相対パスはこの時点のカレントディレクトリ基準で絶対パスに固定する。
    pub fn open(path: &Path, channel: &str, username: &str) -> anyhow::Result<Self> {
        let path = resolve(path)
            .with_context(|| format!("cannot resolve chatters_log path: {}", path.display()))?;
        open_append(&path)
            .with_context(|| format!("cannot open chatters_log: {}", path.display()))?;
        Ok(Self {
            path,
            channel: channel.to_string(),
            username: username.to_string(),
        })
    }

    /// 1 行追記する。毎回開き直すので、追記先が一時的に失われても戻れば次の呼び出しで書ける。
    pub fn append<'a>(
        &self,
        at: chrono::NaiveDateTime,
        logins: impl IntoIterator<Item = &'a str>,
    ) -> std::io::Result<()> {
        let mut line = format_chatters_line(at, logins, &self.channel, &self.username);
        line.push('\n');
        let mut file = open_append(&self.path)?;
        // バッファを挟まず直接書くので、ここで返れば行は OS に渡っている（kill されても残る）
        file.write_all(line.as_bytes())?;
        file.flush()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn open_append(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

/// 相対パスをカレントディレクトリ基準で絶対化する（絶対パスはそのまま）。
fn resolve(path: &Path) -> std::io::Result<PathBuf> {
    std::path::absolute(path)
}

#[cfg(test)]
mod tests {
    use super::{format_chatters_line, ChattersLog};

    fn at(h: u32, mi: u32, s: u32) -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 8, 9)
            .unwrap()
            .and_hms_opt(h, mi, s)
            .unwrap()
    }

    #[test]
    fn formats_timestamp_then_logins_sorted_ascending() {
        let line = format_chatters_line(
            at(12, 34, 56),
            ["zoe", "alice", "bob"],
            "mychannel",
            "mybot",
        );

        assert_eq!(line, "2026-08-09 12:34:56,alice,bob,zoe");
    }

    #[test]
    fn excludes_channel_owner_and_bot_account() {
        let line = format_chatters_line(
            at(0, 0, 0),
            ["mychannel", "mybot", "viewer"],
            "mychannel",
            "mybot",
        );

        assert_eq!(line, "2026-08-09 00:00:00,viewer");
    }

    #[test]
    fn keeps_trailing_comma_when_every_chatter_is_excluded() {
        let line = format_chatters_line(at(0, 0, 0), ["mybot"], "mychannel", "mybot");

        assert_eq!(line, "2026-08-09 00:00:00,");
    }

    #[test]
    fn open_creates_a_missing_file_and_appends_show_chatters_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chatters.csv");

        let log = ChattersLog::open(&path, "mychannel", "mybot").unwrap();
        assert!(path.exists(), "起動時に作られるはず");
        log.append(at(12, 34, 56), ["zoe", "mychannel", "alice", "mybot"])
            .unwrap();
        log.append(at(12, 35, 56), ["mybot"]).unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "2026-08-09 12:34:56,alice,zoe\n2026-08-09 12:35:56,\n"
        );
    }

    #[test]
    fn open_keeps_existing_lines_and_appends_after_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chatters.csv");
        std::fs::write(&path, "2026-08-08 00:00:00,old\n").unwrap();

        let log = ChattersLog::open(&path, "mychannel", "mybot").unwrap();
        log.append(at(0, 0, 0), ["new"]).unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "2026-08-08 00:00:00,old\n2026-08-09 00:00:00,new\n"
        );
    }

    #[test]
    fn open_fails_with_the_os_error_and_the_path_when_the_parent_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-such-dir").join("chatters.csv");

        let err = ChattersLog::open(&path, "mychannel", "mybot")
            .err()
            .expect("親ディレクトリが無ければ開けないはず");

        let msg = format!("{err:#}");
        assert!(msg.contains(&path.display().to_string()), "{msg}");
        let os = std::fs::File::open(&path).unwrap_err().to_string();
        assert!(msg.contains(&os), "{msg} / {os}");
    }

    #[test]
    fn append_retries_on_each_call_so_a_restored_destination_is_written_again() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let path = sub.join("chatters.csv");
        let log = ChattersLog::open(&path, "mychannel", "mybot").unwrap();

        std::fs::remove_dir_all(&sub).unwrap();
        assert!(log.append(at(0, 0, 0), ["a"]).is_err());

        std::fs::create_dir(&sub).unwrap();
        log.append(at(0, 1, 0), ["b"]).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "2026-08-09 00:01:00,b\n"
        );
    }

    #[test]
    fn relative_paths_are_resolved_against_the_current_directory() {
        let resolved = super::resolve(std::path::Path::new("chatters.csv")).unwrap();
        assert_eq!(
            resolved,
            std::env::current_dir().unwrap().join("chatters.csv")
        );
        let dir = tempfile::tempdir().unwrap();
        let abs = dir.path().join("chatters.csv");
        assert_eq!(super::resolve(&abs).unwrap(), abs);
    }
}
