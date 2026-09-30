<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 방언 SQL 검증 — 실제 서버로 재현하는 방법

`src/plugins/postgresKeys.ts`와 `schemaKeys.ts`의 SQL은 **실제 데이터베이스에서 실행해 검증했다.**
컴파일되는 SQL은 검증된 SQL이 아니고, 이 문서는 그 검증을 주장이 아니라 **반복 가능한 절차**로
남기기 위한 것이다.

`scripts/verify-dialect-sql.mjs`는 `npm test`에 들어 있지 않다. 살아 있는 PostgreSQL이 필요하고,
대상이 없을 때 조용히 통과하는 테스트는 테스트가 없는 것보다 나쁘다.

## 왜 이것이 필요했나

이 하네스가 **네 개의 실제 버그를 잡았고**, 그중 둘은 내가 넣은 것이다.

| 검사 | 무엇이 틀렸나 |
|---|---|
| 같은 이름 제약 | 상류가 `constraint_name`으로 묶는다. PostgreSQL은 제약 이름을 **표 단위**로 한정하므로 두 표가 모두 `fk_same`을 가질 수 있고, `getIncomingKeys`는 표를 넘어 훑으므로 **관계 없는 두 키가 가짜 복합 하나로 합쳐진다.** 측정: 한 표를 참조하는 5행이 이름 3개, oid 4개 — 이름으로 묶으면 실제 키 4개가 3개로 줄어든다. `c.oid`로 묶는다 |
| 제약 이름 보고 | 내 `groupForeignKeyRows`가 SQLite 경로에서 가져온 대로 **id를 무조건** 제약 이름으로 보고했다. PostgreSQL 소비자가 `fk_orders_user`를 찾으면 oid를 받았다 |
| 인덱스 열 | `pg_index.indkey`는 하한이 **0인** int2vector다. `generate_subscripts`가 0..n-1을 내는데 내가 일반 배열처럼 1기반이라고 보고 `pos - 1`을 썼다. 결과는 열이 밀리는 게 아니라 **조용한 절단**이다 — `indkey[-1]`이 null이라 조인이 그 행을 버리고, 단일 열 인덱스는 **아예 사라지고** 두 열 인덱스는 첫 열만 남았다 |
| 교차 스키마 | `archive.snapshots`가 `shop.orders`를 참조할 때 `from_schema`를 요청 스키마와 같다고 가정하지 않는지 |

세 번째가 이 문서의 요점이다. **단일 열 인덱스만 있는 픽스처로는 통과했을 것이다.** 두 열 인덱스가
픽스처에 있어야 잡힌다.

## PostgreSQL 서버를 root 없이 띄우기

이 프로젝트는 한때 "이 기계에 PostgreSQL이 없다"고 적고 그 방언을 미루었다. **그것은 측정이 아니라
가정이었다.** 서버는 .deb에서 개인 접두사로 풀어 비특권 사용자로 돈다.

```bash
cd "$SCRATCH"
apt-get download postgresql-18 postgresql-client-18 postgresql-common \
                 postgresql-client-common libpq5 liburing2
for d in libpq5 liburing2 postgresql-18 postgresql-client-18 \
         postgresql-common postgresql-client-common; do
  dpkg-deb -x ${d}_*.deb dbprefix/
done

export PGBIN="$SCRATCH/dbprefix/usr/lib/postgresql/18/bin"
export LD_LIBRARY_PATH="$SCRATCH/dbprefix/usr/lib/x86_64-linux-gnu"
export PGDATA="$SCRATCH/pgdata"

"$PGBIN/initdb" -D "$PGDATA" -U redrob --auth=trust -E UTF8
"$PGBIN/pg_ctl" -D "$PGDATA" -l "$SCRATCH/pg.log" \
  -o "-p 5439 -k $SCRATCH -h 127.0.0.1" start
```

`liburing2`가 필요한 유일한 추가 라이브러리다 — `ldd`가 그것만 not found로 보고한다.
루프백에만 바인드하고, 다른 서비스와 부딪히지 않는 포트를 쓴다.

## 픽스처

아래 스키마가 검증이 의존하는 경우들을 담는다. 각 요소가 특정 버그를 겨냥한다.

```sql
CREATE SCHEMA shop; CREATE SCHEMA archive;

CREATE TABLE shop.users (
  id integer PRIMARY KEY, tenant text NOT NULL, email text,
  CONSTRAINT users_tenant_id_key UNIQUE (tenant, id));

CREATE TABLE shop.orders (
  id integer PRIMARY KEY,
  user_id integer CONSTRAINT fk_orders_user REFERENCES shop.users (id)
    ON DELETE CASCADE ON UPDATE SET NULL,
  tenant text, owner_id integer,
  CONSTRAINT fk_orders_tenant_owner FOREIGN KEY (tenant, owner_id)
    REFERENCES shop.users (tenant, id) ON DELETE RESTRICT);

CREATE TABLE shop.line_items (
  id integer PRIMARY KEY,
  order_id integer CONSTRAINT fk_items_order REFERENCES shop.orders (id));

-- 위치가 불리언이 아니라 서수임을 확인한다
CREATE TABLE shop.ledger (
  entry_date date, sequence integer,
  CONSTRAINT ledger_pkey PRIMARY KEY (entry_date, sequence));

-- 교차 스키마 참조
CREATE TABLE archive.snapshots (
  id integer PRIMARY KEY,
  order_id integer CONSTRAINT fk_snapshots_order REFERENCES shop.orders (id));

-- 두 열 인덱스가 indkey 첨자 버그를 잡는 유일한 요소다
CREATE INDEX idx_orders_user ON shop.orders (user_id);
CREATE UNIQUE INDEX idx_orders_tenant_owner ON shop.orders (tenant, owner_id);

-- 이름이 같은 제약 두 개. PostgreSQL이 허용하고, 상류가 여기서 틀린다
CREATE TABLE shop.a (id integer PRIMARY KEY,
  u integer CONSTRAINT fk_same REFERENCES shop.users (id));
CREATE TABLE shop.b (id integer PRIMARY KEY,
  u integer CONSTRAINT fk_same REFERENCES shop.users (id));
```

## MariaDB 서버를 root 없이 띄우기

PostgreSQL과 같은 방법이고, 없는 라이브러리는 **하나도 없다**.

```bash
cd "$SCRATCH"
apt-get download mariadb-server mariadb-server-core mariadb-client \
                 mariadb-client-core mariadb-common
for d in mariadb-common mariadb-client-core mariadb-client \
         mariadb-server-core mariadb-server; do
  dpkg-deb -x ${d}_*.deb dbprefix/
done

export LD_LIBRARY_PATH="$SCRATCH/dbprefix/usr/lib/x86_64-linux-gnu"
export PATH="$SCRATCH/dbprefix/usr/bin:$SCRATCH/dbprefix/usr/sbin:$PATH"

mariadb-install-db --basedir="$SCRATCH/dbprefix/usr" \
  --datadir="$SCRATCH/mysqldata" --user="$(id -un)" \
  --auth-root-authentication-method=normal

nohup mariadbd --datadir="$SCRATCH/mysqldata" \
  --basedir="$SCRATCH/dbprefix/usr" --socket="$SCRATCH/mysql.sock" \
  --port=3309 --bind-address=127.0.0.1 --pid-file="$SCRATCH/mysql.pid" \
  --skip-grant-tables > "$SCRATCH/mysql.log" 2>&1 &
```

### MySQL 검증이 잡은 것

| 검사 | 무엇이 틀렸나 |
|---|---|
| 들어오는 키의 행 수 | 상류의 `getIncomingKeys`가 `information_schema.referential_constraints`를 **ON 절 없이** 조인한다. 같은 파일의 `getOutgoingKeys`에는 ON 절이 있고 들어오는 쪽 복사본이 그것을 잃었다 — **카테시안 곱**이다. 실측: 한 표를 참조하는 키를 물으면 **16행**이 오고 그중 4행이 옳으며, 나머지는 **무관한 제약의 `on_update`·`on_delete`를 달고 온다** |
| 제약 이름으로 묶기 | **여기서는 상류가 맞다.** PostgreSQL은 제약 이름을 표 단위로 한정해서 이름으로 묶으면 병합되지만, InnoDB는 데이터베이스 단위로 한정하고 중복을 **errno 121로 거부한다**(실측: 두 번째 `fk_same` CREATE가 실패). 같은 패턴이 한 방언에서 버그이고 다른 방언에서 옳다 |

**MySQL 본체가 아니라 MariaDB로 검증했다.** 쿼리가 쓰는 것은
`information_schema.key_column_usage`·`referential_constraints`·`statistics`뿐이고 둘이 같은 형태로
구현하며, 상류 클라이언트도 둘을 한 이름으로 다룬다. 그래도 **가리지 않고 적는다.**

## 아직 검증하지 못한 방언

SQL Server 하나. 그리고 이것은 **가정이 아니라 확인했다**: 배포판 패키지 색인에 `mssql-server`가
**아예 없고**(클라이언트 바인딩만 있다) `docker`도 `podman`도 없다. 앞의 두 번(PostgreSQL, MySQL)은
"서버가 없다"가 가정이었고 둘 다 `.deb`에서 돌았으므로, 이번에는 먼저 찾아보고 적었다.

SQLite는 `node:sqlite`가 내장이므로 별도 서버가 필요 없고 `npm test` 안에서 실제 데이터베이스로
검증된다.

## 실행

```bash
export REDROB_PG_PSQL="$PGBIN/psql"
export REDROB_PG_ARGS="-h 127.0.0.1 -p 5439 -U redrob -d postgres"
export REDROB_MYSQL_CLI="$SCRATCH/dbprefix/usr/bin/mariadb"
export REDROB_MYSQL_ARGS="--socket=$SCRATCH/mysql.sock"
npx tsx scripts/verify-dialect-sql.mjs
```

**33개 검사**가 모두 `ok`여야 한다(PostgreSQL 17 + MySQL 16). 환경변수가 없는 방언은 **건너뛴다고
말하고** 조용히 통과하지 않는다.
