//! One interface language for the whole app, the way Redrob Canvas does it.
//!
//! On first run the language is the system's (`sys_locale`: `GetUserPreferredUILanguages` on
//! Windows, `CFLocaleCopyPreferredLanguages` on macOS, `LANGUAGE` / `LC_ALL` / `LC_MESSAGES` /
//! `LANG` elsewhere; `REDROB_QUERY_LOCALE` overrides all of that). Help > Language sets an override
//! persisted in `<config dir>/language.json` (`{ "language": "auto" | "<code>" }`). Every string
//! the shell draws goes through [`t`] / [`tf`]: English is the source, Korean the one catalog.

use std::cell::Cell;
use std::path::PathBuf;

use crate::services;

/// `REDROB_QUERY_LOCALE=ko-KR`: the system language, for tests and screenshots.
pub const LOCALE_ENV: &str = "REDROB_QUERY_LOCALE";
/// The preference file under the config directory.
pub const PREF_FILE: &str = "language.json";
/// The preference value that follows the system.
pub const AUTO: &str = "auto";

/// A language the shell has strings for.
#[derive(Debug, PartialEq, Eq)]
pub struct LangInfo {
    pub code: &'static str,
    /// The name in that language, for the menu.
    pub name: &'static str,
}

pub const LANGUAGES: &[LangInfo] = &[
    LangInfo {
        code: "en",
        name: "English",
    },
    LangInfo {
        code: "ko",
        name: "한국어",
    },
];

/// A language by code (case-insensitive).
pub fn by_code(code: &str) -> Option<&'static LangInfo> {
    LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(code))
}

pub fn english() -> &'static LangInfo {
    &LANGUAGES[0]
}

/// The language for a locale tag such as `ko_KR.UTF-8`, `ko-KR` or `en-US`: the primary subtag
/// decides. `None` for a language without a catalog and for `C` / `POSIX`, which state no
/// preference (so the next tag decides).
pub fn from_tag(tag: &str) -> Option<&'static LangInfo> {
    let base = tag
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .trim()
        .replace('_', "-")
        .to_ascii_lowercase();
    let primary = base.split('-').find(|p| !p.is_empty())?;
    if matches!(primary, "c" | "posix") {
        return None;
    }
    by_code(primary)
}

/// The first supported language among `tags` (most preferred first); English when none is.
pub fn from_tags<S: AsRef<str>>(tags: &[S]) -> &'static LangInfo {
    tags.iter()
        .find_map(|t| from_tag(t.as_ref()))
        .unwrap_or_else(english)
}

/// The system's language: `REDROB_QUERY_LOCALE`, else the OS preferred-languages list.
pub fn system() -> &'static LangInfo {
    if let Ok(tag) = std::env::var(LOCALE_ENV)
        && !tag.trim().is_empty()
    {
        return from_tags(&[tag]);
    }
    let tags: Vec<String> =
        std::panic::catch_unwind(|| sys_locale::get_locales().take(64).collect())
            .unwrap_or_default();
    from_tags(&tags)
}

fn pref_path() -> Option<PathBuf> {
    services::config_dir()
        .filter(|_| services::prefs_enabled())
        .map(|d| d.join(PREF_FILE))
}

/// The `language` value of `language.json`: a code or `auto`; `auto` for anything unreadable.
pub fn parse_pref(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| {
            v.get("language")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .filter(|p| p.eq_ignore_ascii_case(AUTO) || by_code(p).is_some())
        .unwrap_or_else(|| AUTO.to_owned())
}

fn load_pref() -> String {
    pref_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map_or_else(|| AUTO.to_owned(), |t| parse_pref(&t))
}

fn save_pref(pref: &str) {
    let Some(path) = pref_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = serde_json::json!({ "language": pref }).to_string() + "\n";
    if let Err(e) = std::fs::write(&path, text) {
        log::warn!("couldn't save {}: {e}", path.display());
    }
}

/// The language setting: the persisted preference and the system language it resolves against.
#[derive(Debug)]
pub struct Language {
    pref: String,
    system: &'static LangInfo,
}

impl Language {
    pub fn load() -> Self {
        Self {
            pref: load_pref(),
            system: system(),
        }
    }

    /// The language the app is drawn in.
    pub fn effective(&self) -> &'static LangInfo {
        if self.pref.eq_ignore_ascii_case(AUTO) {
            self.system
        } else {
            by_code(&self.pref).unwrap_or(self.system)
        }
    }

    pub fn is_auto(&self) -> bool {
        self.pref.eq_ignore_ascii_case(AUTO)
    }

    pub fn system(&self) -> &'static LangInfo {
        self.system
    }

    /// Set and persist the preference (`auto` or a code).
    pub fn set(&mut self, pref: &str) {
        self.pref = if pref.eq_ignore_ascii_case(AUTO) || by_code(pref).is_some() {
            pref.to_ascii_lowercase()
        } else {
            AUTO.to_owned()
        };
        save_pref(&self.pref);
        set_current(self.effective());
    }
}

thread_local! {
    static CURRENT: Cell<&'static LangInfo> = const { Cell::new(&LANGUAGES[0]) };
}

/// Set the language the shell's strings are drawn in (at start and on a change).
pub fn set_current(lang: &'static LangInfo) {
    CURRENT.set(lang);
}

pub fn current() -> &'static LangInfo {
    CURRENT.get()
}

/// Translate an English source string into the current language; an unknown string comes back
/// unchanged.
pub fn t(s: &str) -> &str {
    tr(current().code, s)
}

/// [`t`] for an explicit language code.
pub fn tr<'a>(code: &str, s: &'a str) -> &'a str {
    match code {
        "ko" => KO.iter().find(|(en, _)| *en == s).map_or(s, |(_, ko)| ko),
        _ => s,
    }
}

/// `{name}` placeholders in a translated template.
pub fn fmt(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = template.to_owned();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// [`t`] then [`fmt`].
pub fn tf(s: &str, args: &[(&str, &str)]) -> String {
    fmt(t(s), args)
}

/// The Korean catalog (English is the source).
const KO: &[(&str, &str)] = &[
    ("Ask", "Ask"),
    ("Clear", "지우기"),
    ("Open Redrob Launcher", "Redrob Launcher 열기"),
    (
        "{name} · via Redrob Launcher",
        "{name} · Redrob Launcher 계정",
    ),
    ("{name} · this app's own key", "{name} · 이 앱의 자체 키"),
    (
        "Describe the query you want. The active tab's text is sent along as context; result rows never are.",
        "원하는 쿼리를 말로 설명하세요. 현재 탭의 텍스트는 맥락으로 함께 보내지만 결과 행은 절대 보내지 않습니다.",
    ),
    ("You", "나"),
    ("Thinking…", "생각 중…"),
    (
        "e.g. customers who ordered twice this month, newest first",
        "예: 이번 달 두 번 주문한 고객, 최신순",
    ),
    ("Open a query tab first", "먼저 쿼리 탭을 여세요"),
    ("Ask (Ctrl+Enter)", "질문 (Ctrl+Enter)"),
    ("Insert into editor", "편집기에 넣기"),
    ("Copy", "복사"),
    (
        "Not signed in. Sign in with Redrob Launcher and this app will use that account.",
        "로그인되지 않았습니다. Redrob Launcher에서 로그인하면 이 앱이 그 계정을 사용합니다.",
    ),
    (
        "At most {n} tabs are kept; close one first.",
        "탭은 최대 {n}개까지 둘 수 있습니다. 먼저 하나를 닫으세요.",
    ),
    ("Query {n}", "쿼리 {n}"),
    (
        "Select or connect a connection first",
        "먼저 연결을 선택하거나 연결하세요",
    ),
    ("Connected to {name}", "{name}에 연결됨"),
    ("Table change applied", "테이블 변경을 적용했습니다"),
    (
        "Applied {rows} row change(s) in {ms} ms",
        "행 변경 {rows}건을 {ms} ms에 적용했습니다",
    ),
    ("Signed in: {name}", "로그인: {name}"),
    ("Signed out of Redrob", "Redrob에서 로그아웃됨"),
    (
        "Open a query tab to insert the draft into",
        "초안을 넣을 쿼리 탭을 여세요",
    ),
    (
        "Open a query tab first (Ctrl+T)",
        "먼저 쿼리 탭을 여세요 (Ctrl+T)",
    ),
    ("Copied", "복사했습니다"),
    ("File", "파일"),
    ("Connection", "연결"),
    ("Query", "쿼리"),
    ("View", "보기"),
    ("Help", "도움말"),
    ("New tab", "새 탭"),
    ("Close tab", "탭 닫기"),
    ("Open…", "열기…"),
    ("Save", "저장"),
    ("Save as…", "다른 이름으로 저장…"),
    ("Quit", "종료"),
    ("New connection…", "새 연결…"),
    ("Connect {name}", "{name} 연결"),
    ("Disconnect {name}", "{name} 연결 해제"),
    ("Reload schema", "스키마 다시 읽기"),
    ("Edit…", "편집…"),
    ("Remove…", "삭제…"),
    ("Run", "실행"),
    ("Run all", "전체 실행"),
    ("Cancel", "취소"),
    ("Format", "정리"),
    ("Clear result", "결과 지우기"),
    ("Structure panel", "구조 패널"),
    ("History panel", "기록 패널"),
    ("Ask dock", "Ask 도크"),
    ("Shortcuts", "단축키"),
    ("Theme", "테마"),
    ("Language", "언어"),
    ("System", "시스템"),
    ("Light", "밝게"),
    ("Dark", "어둡게"),
    ("Report an issue…", "문제 신고…"),
    ("About {app}", "{app} 정보"),
    ("System ({name})", "시스템 ({name})"),
    (
        "Follow the operating system's language",
        "운영 체제의 언어를 따릅니다",
    ),
    ("{what}  (Ctrl+Shift+A)", "{what}  (Ctrl+Shift+A)"),
    ("Show or hide the Ask dock", "Ask 도크 표시/숨기기"),
    (
        "Signed in with Redrob Launcher",
        "Redrob Launcher로 로그인됨",
    ),
    ("Connect Redrob", "Redrob 연결"),
    (
        "Sign in with Redrob Launcher to use Ask",
        "Ask를 쓰려면 Redrob Launcher에서 로그인하세요",
    ),
    ("read-only", "읽기 전용"),
    ("read-write", "읽기·쓰기"),
    ("connected", "연결됨"),
    ("not connected", "연결 안 됨"),
    ("Running… Esc cancels", "실행 중… Esc로 취소"),
    ("Ready", "준비"),
    (
        "{connected} connected · {tabs} tabs",
        "연결 {connected} · 탭 {tabs}",
    ),
    ("no connection", "연결 없음"),
    ("{what}  (Ctrl+T)", "{what}  (Ctrl+T)"),
    (
        "New query tab on the active connection",
        "현재 연결에 새 쿼리 탭",
    ),
    ("History", "기록"),
    (
        "Click to insert into the editor; double-click opens a tab.",
        "클릭하면 편집기에 넣고, 두 번 클릭하면 탭으로 엽니다.",
    ),
    ("Nothing has run yet.", "아직 실행한 것이 없습니다."),
    (
        "{name} · {rows} rows · {ms} ms",
        "{name} · {rows}행 · {ms} ms",
    ),
    (
        "That connection is not available any more",
        "그 연결은 더 이상 없습니다",
    ),
    ("Connect a database", "데이터베이스에 연결"),
    (
        "Add a PostgreSQL, MySQL, SQLite or MongoDB connection, then query it in tabs.",
        "PostgreSQL, MySQL, SQLite, MongoDB 연결을 추가하고 탭에서 쿼리하세요.",
    ),
    ("New connection", "새 연결"),
    ("Try the {name} database", "{name} 데이터베이스로 시작"),
    ("Version {version}", "버전 {version}"),
    (
        "GPL-3.0-or-later. Third-party notices: NOTICE and UPSTREAM_NOTICES.md.",
        "GPL-3.0-or-later. 서드파티 고지: NOTICE 및 UPSTREAM_NOTICES.md.",
    ),
    (
        "row {n} has no primary key value",
        "{n}행에 기본 키 값이 없습니다",
    ),
    (
        "the primary key column itself cannot be edited",
        "기본 키 열 자체는 편집할 수 없습니다",
    ),
    (
        "the table has no primary key",
        "테이블에 기본 키가 없습니다",
    ),
    (
        "select the primary key column to edit",
        "편집하려면 기본 키 열을 함께 선택하세요",
    ),
    (
        "composite primary keys cannot be edited here yet",
        "복합 기본 키는 아직 여기서 편집할 수 없습니다",
    ),
    (
        "Applied: {rows} rows changed in {ms} ms",
        "적용: {rows}행을 {ms} ms에 변경",
    ),
    (
        "Edits need a result from one table.",
        "편집은 한 테이블의 결과에서만 가능합니다.",
    ),
    (
        "Double-click a cell to edit it; right-click a row number to delete the row. Nothing is written until Apply.",
        "셀을 두 번 클릭해 편집하고, 행 번호를 오른쪽 클릭해 행을 삭제합니다. 적용을 누르기 전에는 아무것도 쓰지 않습니다.",
    ),
    (
        "This connection is read-only. Untick Read-only in its connection form to allow reviewed edits.",
        "이 연결은 읽기 전용입니다. 연결 양식에서 읽기 전용을 해제하면 검토 편집을 할 수 있습니다.",
    ),
    ("Apply", "적용"),
    ("Discard", "버리기"),
    ("row {n}", "{n}행"),
    ("delete", "삭제"),
    (
        "SQL the engine will run on {table}, keyed by {key}, in one transaction:",
        "엔진이 {table}에 {key} 키로 한 트랜잭션으로 실행할 SQL:",
    ),
    ("{rows} rows in {ms} ms", "{rows}행 · {ms} ms"),
    (" · {n} affected", " · {n}행 영향"),
    ("truncated at the limit", "제한에서 잘림"),
    (
        "Choose a connection for this tab first",
        "먼저 이 탭의 연결을 선택하세요",
    ),
    ("Nothing to run", "실행할 것이 없습니다"),
    ("Cancelled", "취소했습니다"),
    (
        "Could not open {path}: {error}",
        "{path}을(를) 열 수 없습니다: {error}",
    ),
    ("Saved {path}", "{path} 저장함"),
    ("Could not save: {error}", "저장할 수 없습니다: {error}"),
    ("▶ Run", "▶ 실행"),
    (
        "Ctrl+Enter: the selection or the statement under the cursor",
        "Ctrl+Enter: 선택 영역 또는 커서가 있는 문장",
    ),
    ("F5: the whole text", "F5: 전체 텍스트"),
    ("Esc", "Esc"),
    ("statement {i} of {n}", "문장 {i}/{n}"),
    ("Limit", "제한"),
    (
        "mutation (edit through the grid)",
        "변경 문장 (그리드에서 편집하세요)",
    ),
    ("destructive (refused)", "파괴적 (거부됨)"),
    ("transaction control (refused)", "트랜잭션 제어 (거부됨)"),
    ("unknown (refused)", "알 수 없음 (거부됨)"),
    (
        "No connection: pick one in the Connection menu",
        "연결 없음: 연결 메뉴에서 선택하세요",
    ),
    ("Grid", "그리드"),
    ("Messages", "메시지"),
    ("Changes", "변경"),
    ("running… {s} s", "실행 중… {s}초"),
    (
        "Run a statement to see its rows here (Ctrl+Enter).",
        "문장을 실행하면 여기에 행이 보입니다 (Ctrl+Enter).",
    ),
    ("No messages yet.", "메시지가 없습니다."),
    ("{s} s ago", "{s}초 전"),
    ("editable", "편집 가능"),
    ("Primary key: {key}", "기본 키: {key}"),
    (
        "Edits need one table with a primary key and a writable connection",
        "편집에는 기본 키가 있는 한 테이블과 쓰기 가능한 연결이 필요합니다",
    ),
    ("Export", "내보내기"),
    ("Copy as TSV", "TSV로 복사"),
    ("Copy as JSON", "JSON으로 복사"),
    ("Save TSV…", "TSV 저장…"),
    ("Save JSON…", "JSON 저장…"),
    ("Columns", "열"),
    ("Show all", "모두 표시"),
    ("Table", "표"),
    ("Filter rows", "행 필터"),
    ("rows {first}–{last}", "{first}–{last}행"),
    (" · {n} shown", " · {n}행 표시"),
    (" · {ms} ms", " · {ms} ms"),
    ("more rows available", "더 많은 행이 있습니다"),
    ("{n} selected", "{n}행 선택"),
    ("Next page", "다음 페이지"),
    ("Previous page", "이전 페이지"),
    ("No columns to show.", "표시할 열이 없습니다."),
    ("{type} · click to sort", "{type} · 클릭해 정렬"),
    ("Copy row as JSON", "행을 JSON으로 복사"),
    ("Copy row as TSV", "행을 TSV로 복사"),
    ("Undo delete", "삭제 취소"),
    ("Delete row", "행 삭제"),
    ("Copy value", "값 복사"),
    ("Edit cell", "셀 편집"),
    ("Set NULL", "NULL로 설정"),
    ("Revert cell", "셀 되돌리기"),
    ("Connections", "연결"),
    ("Reload the connection list", "연결 목록 다시 읽기"),
    ("Filter", "필터"),
    ("Built-in", "내장"),
    ("Your connections", "내 연결"),
    (
        "None yet. Press ＋ to add one.",
        "아직 없습니다. ＋를 눌러 추가하세요.",
    ),
    ("Remove connection", "연결 삭제"),
    (
        "Remove \"{name}\" and its saved password? The database itself is not touched.",
        "\"{name}\"과(와) 저장된 비밀번호를 삭제할까요? 데이터베이스 자체는 건드리지 않습니다.",
    ),
    ("Remove", "삭제"),
    ("New query tab", "새 쿼리 탭"),
    ("Disconnect", "연결 해제"),
    ("Connect", "연결"),
    ("Saved connection \"{name}\"", "연결 \"{name}\" 저장함"),
    ("Edit connection", "연결 편집"),
    ("Name", "이름"),
    ("Database", "데이터베이스"),
    ("Browse…", "찾아보기…"),
    ("Host", "호스트"),
    ("Port", "포트"),
    ("User", "사용자"),
    ("Password", "비밀번호"),
    ("unchanged unless typed", "입력하지 않으면 그대로"),
    ("Options", "옵션"),
    ("Edits", "편집"),
    (
        "Read-only (untick to allow reviewed edits)",
        "읽기 전용 (해제하면 검토 편집 가능)",
    ),
    ("Test", "테스트"),
    (
        "Connect once with these settings and report",
        "이 설정으로 한 번 연결해 결과를 보여줍니다",
    ),
    ("DDL {table}", "DDL {table}"),
    ("Schema", "스키마"),
    (
        "Select a connection to browse its tables.",
        "연결을 선택하면 테이블을 볼 수 있습니다.",
    ),
    (
        "{name} is not connected. Double-click it to connect.",
        "{name}은(는) 연결되지 않았습니다. 두 번 클릭해 연결하세요.",
    ),
    ("Filter tables", "테이블 필터"),
    ("Reload", "다시 읽기"),
    ("Loading…", "읽는 중…"),
    ("No tables or collections.", "테이블이나 컬렉션이 없습니다."),
    ("Select top 100", "상위 100행 조회"),
    ("Copy name", "이름 복사"),
    ("Insert name", "이름 넣기"),
    ("(default)", "(기본)"),
    ("Double-click to insert", "두 번 클릭하면 넣습니다"),
    (
        "Rebuilt from the column metadata; constraints and defaults are not included.",
        "열 메타데이터로 재구성했습니다. 제약과 기본값은 들어 있지 않습니다.",
    ),
    ("Structure", "구조"),
    ("＋ Column", "＋ 열"),
    ("Add a column to this table", "이 테이블에 열 추가"),
    (
        "Select a table in the schema tree.",
        "스키마 트리에서 테이블을 선택하세요.",
    ),
    ("No columns reported.", "열 정보가 없습니다."),
    ("Foreign keys", "외래 키"),
    ("Indexes", "인덱스"),
    ("Create table", "테이블 만들기"),
    ("Add column", "열 추가"),
    ("Column", "열"),
    ("Type", "형식"),
    ("Nullable", "NULL 허용"),
    ("Primary", "기본 키"),
    ("＋ column", "＋ 열"),
    (
        "This connection is read-only; the change will be refused.",
        "이 연결은 읽기 전용이라 변경이 거부됩니다.",
    ),
    (
        "Run the selection or the statement under the cursor",
        "선택 영역 또는 커서가 있는 문장 실행",
    ),
    ("Run the whole text", "전체 텍스트 실행"),
    ("Cancel the run in flight", "실행 중인 쿼리 취소"),
    ("Close the tab", "탭 닫기"),
    ("Go to tab 1 … 9", "1 … 9번 탭으로"),
    ("Format the query", "쿼리 정리"),
    ("Open a .sql file into the tab", "탭에 .sql 파일 열기"),
    (
        "Save the tab's text / save as",
        "탭 텍스트 저장 / 다른 이름으로 저장",
    ),
    (
        "Copy the selected cell, or the selected rows as TSV",
        "선택한 셀 복사, 여러 행이면 TSV로 복사",
    ),
    (
        "Schema tree: insert the name at the caret",
        "스키마 트리: 커서 위치에 이름 넣기",
    ),
];
