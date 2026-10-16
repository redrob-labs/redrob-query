# Redrob Query

[English](./README.md) · **한국어**

Redrob Query는 PostgreSQL, MySQL, SQLite, MongoDB를 위한 에이전트형 데이터베이스 클라이언트입니다.
네이티브 창 하나(Rust, wgpu 위의 egui)에서 기본은 읽기만 하고, 허락한 곳에서만 변경을 검토한 뒤
고치며, 옆에 있는 에이전트가 쿼리 초안을 써 줍니다. JVM도, 브라우저 런타임도, 웹 데모도 없습니다.
로그인은 [Redrob Launcher](https://github.com/redrob-labs/redrob-launcher)에서 한 번만 하면 이 앱이
그 계정을 읽습니다.

워크플로는 DBeaver Community를 참고했고, 엔진의 일부는 Beekeeper Studio Community Edition에서
포팅했습니다([NOTICE](NOTICE), [UPSTREAM_NOTICES.md](UPSTREAM_NOTICES.md) 참고). Eclipse RCP·OSGi·
JDBC·Java·Electron은 들어 있지 않습니다.

## 출하 범위

| 영역 | 기능 |
|---|---|
| 프로필 | 내장 **Demo SQLite**(`:memory:`, 오프라인에서 동작)와 사용자 프로필: 생성·테스트·편집·연결·연결 해제·삭제. 비밀번호는 파일이 아니라 OS 키링에 저장 |
| PostgreSQL / MySQL | 신원을 검증하는 TLS, 스키마별로 묶인 표와 뷰, 열, 기본 키·외래 키, 인덱스, 한계가 정해진 읽기 전용 SQL, 검토 후 편집 |
| SQLite | 파일 프로필(편집을 허락하기 전에는 `mode=ro`), 표, 열, 테이블 값 pragma로 읽는 키와 인덱스, 검토 후 편집 |
| MongoDB | 표준/SRV 프로필, `authSource`, 25개 문서 필드 샘플링이 붙은 컬렉션, 엄격한 `find` / `aggregate` / `explain` 요청 본문, 컬렉션 인덱스, JSON 문서 보기 |
| 편집기 | 연결별 탭, SQL과 Mongo JSON 색칠, Ctrl+Enter로 선택 영역 또는 커서가 있는 문장 실행(읽기 전용 배치는 문장 하나씩), Ctrl+Shift+F 포매팅, Ctrl+S로 텍스트를 파일에 저장, 엔진별 시작 쿼리, Mongo 요청 스트립 |
| 결과 | 가상 스크롤 행, 가져온 페이지 안에서 정렬·필터, 열 표시 설정, 셀 복사, TSV / JSON 복사·저장, 탭별 행 한도 |
| 편집 | 셀을 더블클릭해 새 값을 쌓고, 행 메뉴에서 삭제하고, Changes 패널에서 쌓인 변경을 검토한 뒤 Apply가 한 트랜잭션으로 실행합니다. 어느 문장이라도 정확히 한 행만 바꾸지 않으면 전부 되돌립니다. 단일 열 기본 키가 있는 표 하나의 결과와 읽기 전용이 아닌 프로필이 필요합니다 |
| 구조 | PK / FK / not-null 표시가 붙은 열, 외래 키, 인덱스. 고정된 타입 목록 위의 폼으로 표 만들기·열 추가 |
| Ask | 원하는 쿼리를 말로 설명하면 활성 탭의 텍스트만 맥락으로 함께 가고, 결과 행과 자격 증명은 절대 가지 않습니다. 답의 ` ```sql ` / ` ```mql ` 블록은 한 번 눌러 편집기에 넣습니다 |
| 작업공간 | 시작할 때 최대 30개 탭과 50개 이력을 복원(`workspace.json`), 창 위치는 `ui.ron`. `REDROB_QUERY_NO_PREFS=1`이면 아무것도 남기지 않음 |

이 릴리스는 SQL Server를 지원하지 않습니다. 편집기에 입력한 쓰기 문장은 엔진의 읽기 전용 게이트가
거부합니다. 쓰기 경로는 Changes 패널과 구조 폼 둘뿐입니다.

## 구조

```text
apps/redrob-query  (wgpu 위의 eframe / egui 0.36)
  profiles · schema · editor · grid · changes · structure · Ask 도크
                │  Workspace  (옛 명령 하나당 메서드 하나, Pending 핸들로 답)
crates/redrob-query-core
  DataService  ──  SQLx (PostgreSQL / MySQL / SQLite)  ·  mongodb  ·  reqwest (콘솔)
  query.rs 읽기 전용 분류 · cell_edits.rs · ddl.rs · keys.rs · profile_store.rs · secret.rs
  account.rs: Redrob Launcher의 공용 account.json
```

[아키텍처](docs/ARCHITECTURE.md), [보안](docs/SECURITY.md), [DBeaver 워크플로 맵](docs/DBEAVER-PORTING-MAP.md)을
참고하세요.

## 설치

모든 릴리스는 Redrob Launcher가 읽는 이름으로 다음을 싣습니다.

| 플랫폼 | 자산 |
|---|---|
| Linux x86_64 | `redrob-query-<version>-linux-x86_64.tar.gz`, `redrob-query-<version>-linux-x86_64.deb` |
| macOS arm64 | `redrob-query-<version>-macos-arm64.dmg`, `redrob-query-<version>-macos-arm64.zip` (서명·공증·스테이플) |
| Windows x64 | `redrob-query-<version>-windows-x64-setup.exe` (영어 + 한국어 설치 프로그램), `redrob-query-<version>-windows-x64-portable.zip` (서명) |

여기에 `SHA256SUMS.txt`와 `redrob-launcher.json`이 더해집니다. Windows 설치 프로그램은 사용자별로
`%LOCALAPPDATA%\Programs\Redrob Query`에 설치하며, 설치된 제품은 **ProductName `Redrob Query`**,
실행 파일은 **`bin\redrob-query.exe`**, 설정 > 앱 항목은 `RedrobQuery`(게시자 `Redrob`)입니다. Redrob
Launcher가 이 이름으로 설치 여부를 알아봅니다. 자세한 내용: [installer/windows/README.md](installer/windows/README.md).

## 소스에서 빌드

Rust는 rustup으로 설치합니다. `rust-toolchain.toml`이 1.95.0과 rustfmt, Clippy를 고정합니다. Linux에서는
셸이 X11 / Wayland / GL 개발 헤더와 GTK 3(파일 대화상자)에 링크합니다.

```bash
sudo apt-get install -y libxkbcommon-dev libwayland-dev libx11-dev libxrandr-dev libxi-dev \
  libgl1-mesa-dev libgtk-3-dev libdbus-1-dev
cargo run --release -p redrob-query
```

`redrob-query --version`은 버전을 출력하고 끝나며 디스플레이가 필요 없습니다.

### 검증

```bash
cargo fmt --all -- --check
cargo clippy -p redrob-query -p redrob-query-core --all-targets --locked -- -D warnings
cargo test -p redrob-query-core --locked
python3 scripts/check-brand.py
```

기본 테스트는 격리되어 있고 라이브 서버가 필요한 커넥터 테스트 세 개를 ignored로 보고합니다. 돌리려면
일회용 서비스 URL을 내보내고 명시적으로 켭니다.

```bash
export REDROB_TEST_POSTGRES_URL='postgresql://…'
export REDROB_TEST_MYSQL_URL='mysql://…'
export REDROB_TEST_MONGO_URL='mongodb://…'
export REDROB_TEST_MONGO_DATABASE='redrob_test' # 선택
cargo test -p redrob-query-core --locked -- --ignored
```

### 브랜드 자산

`resources/icons`, `resources/brand`, 설치 프로그램의 `res/` 아래 모든 파일은
`scripts/build-brand-assets.py --design-system <경로>`가 디자인 시스템 배포물에서 만들고
`brand/SOURCES.json`에 sha256으로 고정합니다. `scripts/check-brand.py`(CI와 릴리스)가 손으로 고친 파일이나
고정되지 않은 파일을 거부합니다.

### 릴리스

**Release** 워크플로(`gh workflow run release.yml --ref main -f tag=vX.Y.Z`)가 cargo 바이너리 하나로 세
타깃을 빌드하고, Windows는 작업의 OpenID Connect 로그인으로 Azure Artifact Signing에서 서명하고, macOS는
조직의 Developer ID로 서명·공증하며, 공용 `redrob-labs/.github` 게시 작업이 `SHA256SUMS.txt`와
`redrob-launcher.json`을 쓰고 GitHub Release를 latest로 게시합니다. 태그는 `main` 위에 있어야 하고
`Cargo.toml`의 워크스페이스 버전과 같아야 하며 [CHANGELOG.md](CHANGELOG.md)에 `## <version>` 절이 있어야
합니다. `-f dry_run=true`는 브랜치를 빌드·서명만 하고 아무것도 게시하지 않습니다.

## 로컬 데이터

| 파일 | 내용 |
|---|---|
| `connections.json` | 비밀이 없는 프로필 메타데이터(엔진의 파일, 원자적, 저널링) |
| `account.json` | 런처의 Connect나 사용자가 써 둔 이 앱 자체의 Redrob 로그인. 공용 파일보다 우선 |
| `workspace.json` | 열린 탭(쿼리 텍스트만)과 실행 이력. 결과 행·비밀번호·프롬프트는 저장하지 않음 |
| `ui.ron` | eframe의 창 위치와 패널 크기 |
| `language.json` | 도움말 > 언어: `auto` 또는 코드(`en`, `ko`) |

디렉터리는 Windows `%APPDATA%\Redrob Query`, macOS `~/Library/Application Support/Redrob Query`, 그 외
`$XDG_CONFIG_HOME/redrob-query`(`~/.config/redrob-query`)이며 `REDROB_QUERY_CONFIG_DIR`로 바꿀 수 있습니다.
쿼리 텍스트는 평문이라 민감한 리터럴이 들어 있을 수 있습니다. 작업공간과 창 상태를 남기지 않으려면
`REDROB_QUERY_NO_PREFS=1`을 설정하세요.

## 보안 요약

- 비밀이 없는 프로필 메타데이터는 평문 `connections.json`에, 비밀번호는 OS 키링에 저장합니다.
- 비밀이 없는 fsync 저널, 단계적 키링 항목, 배타적 프로필 잠금으로 갱신을 복구 가능하게 하고, 안전하지
  않은 복구나 동시 소유 상황에서는 닫힌 쪽으로 실패합니다.
- 입력한 쿼리는 읽기만 합니다. 매 실행은 읽기 전용 문장만 받고(여러 개는 전부 읽기 전용일 때만, 각각 따로
  실행), PostgreSQL / MySQL은 읽기 전용 트랜잭션, SQLite는 닫힌 쪽으로 실패하는 PRAGMA 허용목록을
  씁니다. 쓰기는 읽기 전용이 아닌 프로필에서, 모든 값을 바인딩해, 검토한 편집과 구조 폼으로만 일어납니다.
- Mongo 요청은 엄격하고 한계가 있습니다. aggregate의 `$out`과 `$merge`는 재귀적으로 거부합니다.
- PostgreSQL / MySQL TLS를 켜면 공개 신뢰 루트로 인증서 신원을 검증합니다.
- Ask는 프롬프트와 활성 탭의 텍스트를 계정의 bearer 키로 Redrob 콘솔에 보냅니다. 결과 행과 자격 증명은
  붙지 않지만, 두 텍스트에 직접 입력한 민감한 리터럴은 전송됩니다.

전체 [보안 모델과 한계](docs/SECURITY.md)를 읽어 보세요.

## Redrob 계정

Ask는 Redrob Launcher가 이 컴퓨터를 로그인시킨 계정을 씁니다. `$XDG_CONFIG_HOME/redrob-shared`,
`~/Library/Application Support/ai.redrob.shared`, `%APPDATA%\Redrob\Shared`(`REDROB_SHARED_DIR`로 변경)의
`account.json`을 런처의 `docs/shared-account.md` 계약대로 요청마다 읽고, 앱이 열려 있는 동안 지켜봅니다.
환경 변수 `REDROB_API_KEY`, 그다음 이 앱 자체의 `account.json`이 그보다 우선합니다. 키는
`{apiBaseUrl}/chat/completions`에 모델 `auto`로 bearer 토큰으로만 보내며, 콘솔이 거부하면 콘솔의 메시지를
그대로 보여 줍니다. 계정이 없으면 Ask 도크가 그렇다고 말하고 런처를 열어 줍니다. 이 앱에는 자체 로그인이
없습니다.

## 현재 경계

- 자유 형식 쓰기 SQL은 없습니다. 변경은 검토한 편집 집합과 표 폼으로만 갑니다. 고치려면 단일 열 기본 키가
  필요하고, MongoDB 결과와 복합 키는 아직 고칠 수 없습니다.
- 정렬과 필터는 데이터베이스가 아니라 가져온 페이지(탭의 한도까지)에만 적용됩니다.
- 사용자 CA·클라이언트 인증서 UI, SSH 터널, ER 다이어그램, 데이터 전송 파이프라인, 비교·마이그레이션 도구,
  드라이버 마켓플레이스, SQL Server 커넥터는 없습니다.
- Mongo 샘플 메타데이터는 한계가 있는 힌트이지 스키마가 아닙니다.

## 라이선스와 저작자 표시

Redrob Query는 [GNU General Public License, version 3 or later](LICENSE)로 배포합니다. GPL-3.0-or-later인
Beekeeper Studio Community Edition 소스를 들여오기 전까지는 Apache-2.0이었고, 더 강한 카피레프트가 코드와
함께 옵니다. 저작자 표시와 상표 유보는 [NOTICE](NOTICE), 상류 코드 고지는
[UPSTREAM_NOTICES.md](UPSTREAM_NOTICES.md)를 보세요. 서드파티 구성 요소는 각자의 라이선스를 따릅니다.
