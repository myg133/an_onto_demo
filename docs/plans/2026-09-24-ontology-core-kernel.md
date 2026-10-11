# 本体内核 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **事实源**：`docs/design/ontology-platform-spec.md`（唯一事实源）。
> 本文提到的「设计文档 §X」一律指该文件。与之冲突时，**改本文，不改规格**。

**Goal:** 实现 `ontology-store` 存储抽象及其 PostgreSQL 实现，含本体 schema 的版本快照、提案（Proposal）生命周期、双轨审核状态机、双轨视图投影与快照 diff，并为 ActionType / Function 预留模型占位 —— 不含 LLM、不含外部数据源接入。

**Architecture:** 领域逻辑（快照结构、diff、审核状态机、视图投影）写成不依赖数据库的纯函数与 Pydantic 模型；持久化走 `OntologyStore` 抽象，两个实现（内存 / PostgreSQL）跑同一套契约测试。所有对外结构用 Pydantic 模型表达，ORM 模型只在 `PostgresOntologyStore` 内部出现，不泄漏到调用方。

**Tech Stack:** Python 3.12 / SQLAlchemy 2.0 / psycopg 3 / Pydantic v2 / pydantic-settings / PostgreSQL 16 / pytest 8 / ruff

---

## 本计划在切片 1 中的位置

设计规格 §8.1 的切片 1（只读闭环）拆为三份计划：

1. **本计划 — 本体内核**：存储、版本、提案、审核、diff。纯库代码，pytest 全量验证
2. **数据接入与抽取**：DB connector 内省、语义契约、抽取引擎产出提案。依赖本计划
3. **审核界面与 MCP 接入**：双轨审核 UI、`mcp-server`、端到端验收。依赖 1 与 2

三份计划各自产出可独立测试的软件。本计划先做，因为它不依赖任何未定项。

> **⚠ 关于执行顺序（2026-09-28 记录）**：本计划覆盖的全是确定性工程（版本、diff、状态机、投影），而项目的核心赌注 —— LLM 能否从真实客户库抽出客户认得的候选本体（设计规格 §2.3）—— 在第 2 份计划里一行代码没写。
> **建议先花 2 天做一个不依赖本计划的 spike**（一个脚本 + 一个 prompt + 一张真实库的 schema 与样本行 → 人工肉眼判断候选准不准），再决定本计划要建多厚。治理机制无法救场一个客户不认的本体。
> 这不影响本计划自身的正确性，只影响投入顺序。

**技术栈假设**：设计规格未锁定语言。本计划选 Python，依据是设计规格 §4.5 依赖 MCP 接入面，而 MCP 官方 Python SDK 最成熟；且数据源内省、LLM 编排生态在 Python 侧最完整。若此假设不成立（例如团队栈是 TypeScript/Java），本计划整体作废，需要重写 —— 请在开工前确认。

**明确不在本计划范围**（后续计划或更晚）：

- Alembic 迁移（本计划用 `Base.metadata.create_all`；迁移在首次部署前引入）
- HTTP API（属于第 3 份计划的接入面）
- 任何 LLM 调用
- connector 与采集器
- 数据面 pipeline（实例数据同步）
- **ActionType / Function 的业务逻辑**（切片 2）—— 本计划只建结构占位，见 Task 3.5

---

## 文件结构

```
pyproject.toml                      依赖与工具配置
src/ontology/__init__.py            包
src/ontology/config.py              Settings（环境变量 ONTO_*）
src/ontology/db.py                  engine / session factory 构造
src/ontology/ids.py                 主键类型与 id 生成
src/ontology/models/__init__.py     汇总导入（保证 create_all 看到全部表）
src/ontology/models/base.py         DeclarativeBase、时间戳 mixin
src/ontology/models/project.py      Project
src/ontology/models/ontology.py     OntologyVersion / EntityType / EntityProperty / LinkType
                                    + ActionType / Function（切片 1 只占位，见 Task 3.5）
src/ontology/models/proposal.py     Proposal / ProposalItem / ReviewRecord
src/ontology/hierarchy.py           类层级的纯函数：祖先 / 后代 / 环检测 / 悬空父检测
src/ontology/snapshot.py            中立快照结构（Pydantic）—— 存储层与领域层之间唯一的数据契约
src/ontology/store/__init__.py      导出 OntologyStore / 两个实现
src/ontology/store/base.py          OntologyStore 抽象接口 + VersionInfo
src/ontology/store/memory.py        InMemoryOntologyStore
src/ontology/store/pg.py            PostgresOntologyStore
src/ontology/review/__init__.py     导出
src/ontology/review/state.py        Track / Decision 枚举、决策写入、依赖检查、提案状态汇总
src/ontology/review/tracks.py       双轨视图投影（biz_view / model_view）
src/ontology/review/apply.py        提案应用与版本冻结
src/ontology/diff.py                快照 diff
tests/conftest.py                   fixture：pg_url / pg_engine / store
tests/test_db.py                    engine 连通性
tests/test_project_model.py         Project 约束
tests/test_ontology_models.py       本体 schema 约束
tests/test_reserved_models.py       ActionType / Function 占位结构
tests/test_class_structure.py       parent / aliases / disjoint_with 的模型约束
tests/test_hierarchy.py             层级纯函数：祖先、后代、环检测
tests/test_store_contract.py        存储契约测试（memory + pg 共用）
tests/test_proposal_models.py       提案模型约束
tests/test_review_state.py          审核状态机
tests/test_tracks.py                双轨视图
tests/test_diff.py                  diff 引擎
tests/test_apply.py                 提案应用与冻结
```

**分层规则**（后续计划必须遵守）：

- `snapshot.py` 是存储层与领域层之间唯一的数据契约。领域代码只认 Pydantic 快照，不 import ORM
- ORM 模型只在 `store/pg.py` 内被 import
- `review/`、`diff.py` 里的核心逻辑是纯函数，接收快照/提案对象，不碰数据库

---

## Task 1: 项目骨架与数据库基础设施

**Files:**
- Create: `pyproject.toml`
- Create: `src/ontology/__init__.py`
- Create: `src/ontology/config.py`
- Create: `src/ontology/db.py`
- Create: `src/ontology/models/__init__.py`
- Create: `src/ontology/models/base.py`
- Create: `tests/conftest.py`
- Create: `tests/test_db.py`

- [ ] **Step 1: 初始化仓库与目录**

仓库尚未纳入版本控制，`git init` 是本计划所有 commit 步骤的前提。

```bash
cd /home/myg133/Projects/Anchnet/Ontology
git init
printf '%s\n' '__pycache__/' '*.pyc' '.pytest_cache/' '.ruff_cache/' '.venv/' '.env' '.codewhale/' '*.egg-info/' > .gitignore
git add .gitignore
git commit -m "chore: init repository"
```

- [ ] **Step 2: 写 pyproject.toml**

Create `pyproject.toml`:

```toml
[project]
name = "ontology-platform"
version = "0.1.0"
description = "FDE 本体论平台"
requires-python = ">=3.12"
dependencies = [
    "sqlalchemy>=2.0.30",
    "psycopg[binary]>=3.1.19",
    "pydantic>=2.8",
    "pydantic-settings>=2.4",
]

[project.optional-dependencies]
dev = ["pytest>=8.2", "ruff>=0.6"]

[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[tool.hatch.build.targets.wheel]
packages = ["src/ontology"]

[tool.pytest.ini_options]
testpaths = ["tests"]
pythonpath = ["src"]
addopts = "-q"

[tool.ruff]
line-length = 100
target-version = "py312"
```

`pythonpath = ["src"]` 让测试无需安装即可 import，省掉一次 `pip install -e .` 的往返。

- [ ] **Step 3: 建虚拟环境并安装依赖**

```bash
cd /home/myg133/Projects/Anchnet/Ontology
python -m venv .venv
.venv/bin/python -m pip install --upgrade pip
.venv/bin/python -m pip install -e ".[dev]"
```

Expected: 安装成功，无报错。

- [ ] **Step 4: 写配置与数据库构造**

Create `src/ontology/__init__.py`:

```python
"""FDE 本体论平台。"""

__version__ = "0.1.0"
```

Create `src/ontology/config.py`:

```python
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    """运行时配置。环境变量前缀 ONTO_，例如 ONTO_DATABASE_URL。"""

    model_config = SettingsConfigDict(env_prefix="ONTO_", env_file=".env", extra="ignore")

    database_url: str = "postgresql+psycopg://onto:onto@localhost:5432/onto"
    test_database_url: str | None = None
```

Create `src/ontology/db.py`:

```python
from sqlalchemy import Engine, create_engine
from sqlalchemy.orm import Session, sessionmaker


def make_engine(database_url: str) -> Engine:
    """构造 engine。pool_pre_ping 让长连接池在数据库重启后能自愈。"""
    return create_engine(database_url, pool_pre_ping=True, future=True)


def make_session_factory(engine: Engine) -> sessionmaker[Session]:
    """expire_on_commit=False：提交后对象属性仍可读，避免 Commit 后意外触发懒加载。"""
    return sessionmaker(bind=engine, expire_on_commit=False, future=True)
```

- [ ] **Step 5: 写 ORM 基类**

Create `src/ontology/models/base.py`:

```python
from datetime import datetime, timezone

from sqlalchemy import DateTime
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column


def utcnow() -> datetime:
    """所有时间戳统一写 UTC。"""
    return datetime.now(timezone.utc)


class Base(DeclarativeBase):
    pass


class TimestampMixin:
    """给每张表加 created_at。"""

    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=utcnow, nullable=False
    )
```

Create `src/ontology/models/__init__.py`:

```python
from ontology.models.base import Base, TimestampMixin, utcnow

__all__ = ["Base", "TimestampMixin", "utcnow"]
```

后续 Task 会往这个 `__init__.py` 追加模型导入 —— 这是让 `Base.metadata.create_all` 看到所有表的关键，漏掉会导致建表不全。

- [ ] **Step 6: 写 conftest.py**

Create `tests/conftest.py`:

```python
import os

import pytest

from ontology.config import Settings
from ontology.db import make_engine


@pytest.fixture(scope="session")
def settings() -> Settings:
    return Settings()


@pytest.fixture(scope="session")
def pg_url(settings) -> str | None:
    """测试库地址。未配置时返回 None 而非 skip —— 让不依赖 PG 的测试照常运行。"""
    return settings.test_database_url or os.environ.get("ONTO_TEST_DATABASE_URL")


@pytest.fixture
def pg_engine(pg_url):
    """每个测试一套干净的表。DDL 全量重建，慢但无状态残留。"""
    if pg_url is None:
        pytest.skip("未设置 ONTO_TEST_DATABASE_URL，跳过 PostgreSQL 测试")

    from ontology.models import Base

    engine = make_engine(pg_url)
    Base.metadata.drop_all(engine)
    Base.metadata.create_all(engine)
    yield engine
    engine.dispose()
```

- [ ] **Step 7: 写连通性测试**

Create `tests/test_db.py`:

```python
from sqlalchemy import text

from ontology.db import make_engine


def test_engine_connects(pg_engine):
    with pg_engine.connect() as conn:
        assert conn.execute(text("select 1")).scalar_one() == 1
```

- [ ] **Step 8: 准备测试库**

```bash
createdb onto_test
export ONTO_TEST_DATABASE_URL="postgresql+psycopg://onto:onto@localhost:5432/onto_test"
```

Expected: `createdb` 成功。若本机没有 PostgreSQL，起一个容器：

```bash
docker run -d --name onto-pg -e POSTGRES_USER=onto -e POSTGRES_PASSWORD=onto -e POSTGRES_DB=onto_test -p 5432:5432 postgres:16
```

- [ ] **Step 9: 跑测试**

Run: `.venv/bin/python -m pytest tests/test_db.py -v`

Expected: PASS（1 passed）。若未设置 `ONTO_TEST_DATABASE_URL`，会是 SKIPPED —— 那是配置问题，不是代码问题，先修配置。

- [ ] **Step 10: Commit**

```bash
git add pyproject.toml src/ontology tests
git commit -m "chore: scaffold ontology platform with db infrastructure"
```

---

## Task 2: Project 模型

项目是一等公民（设计文档 §7.2）：SaaS 上并行跑多个客户项目，项目间强隔离。所有本体、提案、审核记录都挂在项目下，所以它必须是最先落地的实体。

**Files:**
- Create: `src/ontology/models/project.py`
- Modify: `src/ontology/models/__init__.py`
- Create: `tests/test_project_model.py`

- [ ] **Step 1: 写失败的测试**

Create `tests/test_project_model.py`:

```python
import pytest
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session

from ontology.models.project import Project


def test_project_is_persisted_with_uuid_and_timestamp(pg_engine):
    with Session(pg_engine) as session:
        project = Project(name="acme", description="ACME 设备运维")
        session.add(project)
        session.commit()
        project_id = project.id
        created_at = project.created_at

    with Session(pg_engine) as session:
        loaded = session.get(Project, project_id)
        assert loaded is not None
        assert loaded.name == "acme"
        assert loaded.description == "ACME 设备运维"
        assert created_at is not None


def test_project_name_is_unique(pg_engine):
    with Session(pg_engine) as session:
        session.add(Project(name="acme"))
        session.commit()
        session.add(Project(name="acme"))
        with pytest.raises(IntegrityError):
            session.commit()
```

- [ ] **Step 2: 跑测试确认失败**

Run: `.venv/bin/python -m pytest tests/test_project_model.py -v`

Expected: FAIL — `ModuleNotFoundError: No module named 'ontology.models.project'`

- [ ] **Step 3: 实现模型**

Create `src/ontology/models/project.py`:

```python
import uuid

from sqlalchemy import String, Uuid
from sqlalchemy.orm import Mapped, mapped_column

from ontology.models.base import Base, TimestampMixin


class Project(Base, TimestampMixin):
    """一个客户交付项目。项目间强隔离，项目内不做细粒度权限。"""

    __tablename__ = "project"

    id: Mapped[uuid.UUID] = mapped_column(Uuid, primary_key=True, default=uuid.uuid4)
    name: Mapped[str] = mapped_column(String(200), unique=True, nullable=False)
    description: Mapped[str | None] = mapped_column(String(2000))
```

- [ ] **Step 4: 注册到 models 包**

Modify `src/ontology/models/__init__.py` — 改成：

```python
from ontology.models.base import Base, TimestampMixin, utcnow
from ontology.models.project import Project

__all__ = ["Base", "Project", "TimestampMixin", "utcnow"]
```

- [ ] **Step 5: 跑测试确认通过**

Run: `.venv/bin/python -m pytest tests/test_project_model.py -v`

Expected: PASS（2 passed）

- [ ] **Step 6: Commit**

```bash
git add src/ontology/models tests/test_project_model.py
git commit -m "feat: add project model"
```

---

## Task 3: 本体 schema 模型

本体的 schema 层：版本、实体类型、属性、关系类型。设计文档 §6.3 要求"快照 + 冻结"，所以**所有 schema 元素都挂在某个 `OntologyVersion` 下**，而不是直接属于项目 —— 版本是变更的单位。

**Files:**
- Create: `src/ontology/models/ontology.py`
- Modify: `src/ontology/models/__init__.py`
- Create: `tests/test_ontology_models.py`

- [ ] **Step 1: 写失败的测试**

Create `tests/test_ontology_models.py`:

```python
import pytest
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session

from ontology.models.ontology import EntityProperty, EntityType, LinkType, OntologyVersion
from ontology.models.project import Project


@pytest.fixture
def project_id(pg_engine) -> str:
    with Session(pg_engine) as session:
        project = Project(name="acme")
        session.add(project)
        session.commit()
        return project.id


def test_version_seq_is_unique_per_project(pg_engine, project_id):
    with Session(pg_engine) as session:
        session.add(OntologyVersion(project_id=project_id, seq=1))
        session.commit()
        session.add(OntologyVersion(project_id=project_id, seq=1))
        with pytest.raises(IntegrityError):
            session.commit()


def test_version_defaults_to_draft(pg_engine, project_id):
    with Session(pg_engine) as session:
        version = OntologyVersion(project_id=project_id, seq=1)
        session.add(version)
        session.commit()
        assert version.status == "draft"
        assert version.frozen_at is None


def test_entity_type_name_is_unique_within_version(pg_engine, project_id):
    with Session(pg_engine) as session:
        version = OntologyVersion(project_id=project_id, seq=1)
        session.add(version)
        session.commit()
        session.add(EntityType(version_id=version.id, name="设备"))
        session.commit()
        session.add(EntityType(version_id=version.id, name="设备"))
        with pytest.raises(IntegrityError):
            session.commit()


def test_same_entity_type_name_allowed_in_different_versions(pg_engine, project_id):
    with Session(pg_engine) as session:
        v1 = OntologyVersion(project_id=project_id, seq=1)
        v2 = OntologyVersion(project_id=project_id, seq=2)
        session.add_all([v1, v2])
        session.commit()
        session.add_all(
            [
                EntityType(version_id=v1.id, name="设备"),
                EntityType(version_id=v2.id, name="设备"),
            ]
        )
        session.commit()


def test_property_belongs_to_entity_type_and_is_unique(pg_engine, project_id):
    with Session(pg_engine) as session:
        version = OntologyVersion(project_id=project_id, seq=1)
        session.add(version)
        session.commit()
        entity = EntityType(version_id=version.id, name="设备", pk_columns=["dev_id"])
        session.add(entity)
        session.commit()

        session.add(EntityProperty(entity_type_id=entity.id, name="sn", data_type="text"))
        session.commit()
        session.add(EntityProperty(entity_type_id=entity.id, name="sn", data_type="text"))
        with pytest.raises(IntegrityError):
            session.commit()


def test_entity_type_pk_columns_defaults_to_empty_list(pg_engine, project_id):
    with Session(pg_engine) as session:
        version = OntologyVersion(project_id=project_id, seq=1)
        session.add(version)
        session.commit()
        entity = EntityType(version_id=version.id, name="设备")
        session.add(entity)
        session.commit()
        assert entity.pk_columns == []


def test_link_type_references_both_ends(pg_engine, project_id):
    with Session(pg_engine) as session:
        version = OntologyVersion(project_id=project_id, seq=1)
        session.add(version)
        session.commit()
        device = EntityType(version_id=version.id, name="设备")
        ticket = EntityType(version_id=version.id, name="工单")
        session.add_all([device, ticket])
        session.commit()

        link = LinkType(
            version_id=version.id,
            name="设备-工单",
            from_entity_type_id=device.id,
            to_entity_type_id=ticket.id,
            cardinality="one_to_many",
        )
        session.add(link)
        session.commit()
        assert link.from_entity_type_id == device.id
```

- [ ] **Step 2: 跑测试确认失败**

Run: `.venv/bin/python -m pytest tests/test_ontology_models.py -v`

Expected: FAIL — `ModuleNotFoundError: No module named 'ontology.models.ontology'`

- [ ] **Step 3: 实现模型**

Create `src/ontology/models/ontology.py`:

```python
import uuid
from datetime import datetime

from sqlalchemy import (
    Boolean,
    CheckConstraint,
    DateTime,
    ForeignKey,
    Integer,
    String,
    UniqueConstraint,
    Uuid,
)
from sqlalchemy.dialects.postgresql import JSONB
from sqlalchemy.orm import Mapped, mapped_column

from ontology.models.base import Base, TimestampMixin

VERSION_STATUSES = ("draft", "frozen")
CARDINALITIES = ("one_to_one", "one_to_many", "many_to_one", "many_to_many")


class OntologyVersion(Base, TimestampMixin):
    """本体的一次快照。冻结后不可再写入，后续变更只能开新版本。"""

    __tablename__ = "ontology_version"
    __table_args__ = (
        UniqueConstraint("project_id", "seq", name="uq_version_project_seq"),
        CheckConstraint(
            "status in ('draft', 'frozen')", name="ck_version_status"
        ),
    )

    id: Mapped[uuid.UUID] = mapped_column(Uuid, primary_key=True, default=uuid.uuid4)
    project_id: Mapped[uuid.UUID] = mapped_column(
        Uuid, ForeignKey("project.id", ondelete="CASCADE"), nullable=False, index=True
    )
    seq: Mapped[int] = mapped_column(Integer, nullable=False)
    status: Mapped[str] = mapped_column(String(20), nullable=False, default="draft")
    parent_version_id: Mapped[uuid.UUID | None] = mapped_column(
        Uuid, ForeignKey("ontology_version.id", ondelete="SET NULL")
    )
    frozen_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    note: Mapped[str | None] = mapped_column(String(2000))


class EntityType(Base, TimestampMixin):
    """实体类型。evidence 记录它从哪些数据源字段推断而来 —— 没有证据链，人无法有效审核。"""

    __tablename__ = "entity_type"
    __table_args__ = (
        UniqueConstraint("version_id", "name", name="uq_entity_type_version_name"),
    )

    id: Mapped[uuid.UUID] = mapped_column(Uuid, primary_key=True, default=uuid.uuid4)
    version_id: Mapped[uuid.UUID] = mapped_column(
        Uuid, ForeignKey("ontology_version.id", ondelete="CASCADE"), nullable=False, index=True
    )
    name: Mapped[str] = mapped_column(String(200), nullable=False)
    description: Mapped[str | None] = mapped_column(String(2000))
    # 主键列名列表。列名是 primary_key，属性名叫 pk_columns 是为了不与 SQLAlchemy 的
    # Mapper.primary_key 撞名。
    pk_columns: Mapped[list[str]] = mapped_column(
        "primary_key", JSONB, nullable=False, default=list
    )
    evidence: Mapped[list[dict]] = mapped_column(JSONB, nullable=False, default=list)
    # 保存时的顺序。created_at 在同一事务内写入的多行可能落进同一微秒，
    # 靠它排序读回顺序不确定，契约测试会随机失败。
    ordinal: Mapped[int] = mapped_column(Integer, nullable=False, default=0)


class EntityProperty(Base, TimestampMixin):
    """实体类型的属性。"""

    __tablename__ = "entity_property"
    __table_args__ = (
        UniqueConstraint("entity_type_id", "name", name="uq_property_entity_name"),
    )

    id: Mapped[uuid.UUID] = mapped_column(Uuid, primary_key=True, default=uuid.uuid4)
    entity_type_id: Mapped[uuid.UUID] = mapped_column(
        Uuid, ForeignKey("entity_type.id", ondelete="CASCADE"), nullable=False, index=True
    )
    name: Mapped[str] = mapped_column(String(200), nullable=False)
    data_type: Mapped[str] = mapped_column(String(50), nullable=False)
    nullable: Mapped[bool] = mapped_column(Boolean, nullable=False, default=True)
    source_ref: Mapped[dict] = mapped_column(JSONB, nullable=False, default=dict)
    evidence: Mapped[list[dict]] = mapped_column(JSONB, nullable=False, default=list)
    ordinal: Mapped[int] = mapped_column(Integer, nullable=False, default=0)


class LinkType(Base, TimestampMixin):
    """实体类型之间的关系。两端都指向同一版本内的实体类型。"""

    __tablename__ = "link_type"
    __table_args__ = (
        UniqueConstraint("version_id", "name", name="uq_link_type_version_name"),
    )

    id: Mapped[uuid.UUID] = mapped_column(Uuid, primary_key=True, default=uuid.uuid4)
    version_id: Mapped[uuid.UUID] = mapped_column(
        Uuid, ForeignKey("ontology_version.id", ondelete="CASCADE"), nullable=False, index=True
    )
    name: Mapped[str] = mapped_column(String(200), nullable=False)
    from_entity_type_id: Mapped[uuid.UUID] = mapped_column(
        Uuid, ForeignKey("entity_type.id", ondelete="CASCADE"), nullable=False
    )
    to_entity_type_id: Mapped[uuid.UUID] = mapped_column(
        Uuid, ForeignKey("entity_type.id", ondelete="CASCADE"), nullable=False
    )
    cardinality: Mapped[str] = mapped_column(String(20), nullable=False)
    evidence: Mapped[list[dict]] = mapped_column(JSONB, nullable=False, default=list)
    ordinal: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
```

- [ ] **Step 4: 注册到 models 包**

Modify `src/ontology/models/__init__.py` — 改成：

```python
from ontology.models.base import Base, TimestampMixin, utcnow
from ontology.models.ontology import EntityProperty, EntityType, LinkType, OntologyVersion
from ontology.models.project import Project

__all__ = [
    "Base",
    "EntityProperty",
    "EntityType",
    "LinkType",
    "OntologyVersion",
    "Project",
    "TimestampMixin",
    "utcnow",
]
```

- [ ] **Step 5: 跑测试确认通过**

Run: `.venv/bin/python -m pytest tests/test_ontology_models.py -v`

Expected: PASS（7 passed）

- [ ] **Step 6: 跑全量测试确认没打破既有测试**

Run: `.venv/bin/python -m pytest -v`

Expected: PASS（全部通过）

- [ ] **Step 7: Commit**

```bash
git add src/ontology/models tests/test_ontology_models.py
git commit -m "feat: add ontology schema models with versioning"
```

---

## Task 3.5: ActionType / Function 占位模型

> **2026-09-28 新增。本 Task 是设计规格 §4.6 的硬性要求，不可省略。**

设计规格 §4.6 规定：本体核心模型有**四根柱子**（对象 / 关系 / **动作** / **函数**）与三件治理物。切片 1 不实现 Action 与 Function 的业务逻辑，**但模型必须占位**。

**为什么不能省**：`OntologySnapshot` 是存储层与领域层之间的**唯一数据契约**，提案应用（`apply_items`）、快照 diff、双轨视图投影都直接消费它。若本 Task 不做，切片 2 引入 ActionType 时必须回头改动快照结构、diff 引擎、apply 逻辑与双轨视图 —— 这正是设计规格反复警告的「推翻重来」。

**本 Task 只做结构，不做行为**：建表、建快照字段、建占位枚举。切片 1 的抽取引擎**不产出**这两类条目，apply 逻辑不执行它们。

**Files:**
- Create: `src/ontology/models/reserved.py`（或并入 `ontology.py`，视 Task 3 的组织方式）
- Modify: `src/ontology/models/__init__.py`
- Create: `tests/test_reserved_models.py`
- Modify: `src/ontology/snapshot.py`（Task 4 建快照时并入，本 Task 先留 TODO 指向）

- [ ] **Step 1: 写失败的测试**

Create `tests/test_reserved_models.py`:

```python
import pytest
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session

from ontology.models.ontology import ActionType, Function, OntologyVersion
from ontology.models.project import Project


@pytest.fixture
def version_id(pg_engine) -> str:
    with Session(pg_engine) as session:
        project = Project(name="acme")
        session.add(project)
        session.commit()
        version = OntologyVersion(project_id=project.id, seq=1)
        session.add(version)
        session.commit()
        return version.id


def test_action_type_is_persistable_with_reserved_fields(pg_engine, version_id):
    """切片 1 不产生 ActionType，但结构必须能存 —— 否则切片 2 要改表。"""
    with Session(pg_engine) as session:
        action = ActionType(
            version_id=version_id,
            key="dispatch_ticket",
            parameters=[{"name": "ticket_id", "type": "string"}],
            preconditions=[{"expr": "status == 'open'"}],
            effects=[{"system": "wms", "op": "create_task"}],
            permission={"roles": ["planner"]},
            audit_policy={"retain_days": 365},
        )
        session.add(action)
        session.commit()
        assert action.parameters[0]["name"] == "ticket_id"
        assert action.effects[0]["system"] == "wms"


def test_action_type_key_is_unique_within_version(pg_engine, version_id):
    with Session(pg_engine) as session:
        session.add(ActionType(version_id=version_id, key="dispatch_ticket"))
        session.commit()
        session.add(ActionType(version_id=version_id, key="dispatch_ticket"))
        with pytest.raises(IntegrityError):
            session.commit()


def test_same_action_key_allowed_in_different_versions(pg_engine, version_id):
    with Session(pg_engine) as session:
        project_id = session.get(OntologyVersion, version_id).project_id
        v2 = OntologyVersion(project_id=project_id, seq=2)
        session.add(v2)
        session.commit()
        session.add_all([
            ActionType(version_id=version_id, key="dispatch_ticket"),
            ActionType(version_id=v2.id, key="dispatch_ticket"),
        ])
        session.commit()


def test_function_is_persistable(pg_engine, version_id):
    with Session(pg_engine) as session:
        fn = Function(
            version_id=version_id,
            key="device_health_score",
            signature="(device: Device) -> float",
            allowed_types=["Device"],
        )
        session.add(fn)
        session.commit()
        assert fn.allowed_types == ["Device"]


def test_function_key_is_unique_within_version(pg_engine, version_id):
    with Session(pg_engine) as session:
        session.add(Function(version_id=version_id, key="device_health_score"))
        session.commit()
        session.add(Function(version_id=version_id, key="device_health_score"))
        with pytest.raises(IntegrityError):
            session.commit()
```

> 注：后两个测试的 `pg_engine` 参数是为了拿到 fixture —— 它由 `tests/conftest.py` 的 `pg_engine` 提供（每个测试一套干净表）。`version_id` 依赖它，故一并声明。

- [ ] **Step 2: 实现 ORM 模型**

在 `src/ontology/models/ontology.py` 追加（与 `EntityType` 同构，同样挂 `version_id` 与 `evidence`）：

```python
class ActionType(Base, TimestampMixin):
    """对本体对象的写入。切片 1 只占位，不实现。

    字段对应设计规格 §4.6 的四根柱子之一「动作」：
    preconditions 是提交判据，effects 描述写回哪个外部系统的什么。
    """

    __tablename__ = "action_type"
    __table_args__ = (
        UniqueConstraint("version_id", "key", name="uq_action_version_key"),
    )

    id: Mapped[uuid.UUID] = mapped_column(Uuid, primary_key=True, default=uuid.uuid4)
    version_id: Mapped[uuid.UUID] = mapped_column(
        Uuid, ForeignKey("ontology_version.id", ondelete="CASCADE"), nullable=False, index=True
    )
    key: Mapped[str] = mapped_column(String(200), nullable=False)
    target_entity_type_id: Mapped[uuid.UUID | None] = mapped_column(
        Uuid, ForeignKey("entity_type.id", ondelete="CASCADE")
    )
    parameters: Mapped[list[dict]] = mapped_column(JSONB, nullable=False, default=list)
    preconditions: Mapped[list[dict]] = mapped_column(JSONB, nullable=False, default=list)
    effects: Mapped[list[dict]] = mapped_column(JSONB, nullable=False, default=list)
    permission: Mapped[dict] = mapped_column(JSONB, nullable=False, default=dict)
    audit_policy: Mapped[dict] = mapped_column(JSONB, nullable=False, default=dict)
    evidence: Mapped[list[dict]] = mapped_column(JSONB, nullable=False, default=list)
    ordinal: Mapped[int] = mapped_column(Integer, nullable=False, default=0)


class Function(Base, TimestampMixin):
    """派生属性 / 聚合，不改变状态。切片 1 只占位，不实现。"""

    __tablename__ = "function"
    __table_args__ = (
        UniqueConstraint("version_id", "key", name="uq_function_version_key"),
    )

    id: Mapped[uuid.UUID] = mapped_column(Uuid, primary_key=True, default=uuid.uuid4)
    version_id: Mapped[uuid.UUID] = mapped_column(
        Uuid, ForeignKey("ontology_version.id", ondelete="CASCADE"), nullable=False, index=True
    )
    key: Mapped[str] = mapped_column(String(200), nullable=False)
    signature: Mapped[str] = mapped_column(String(2000), nullable=False, default="")
    allowed_types: Mapped[list[str]] = mapped_column(JSONB, nullable=False, default=list)
    evidence: Mapped[list[dict]] = mapped_column(JSONB, nullable=False, default=list)
    ordinal: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
```

同步追加到 `src/ontology/models/__init__.py` 的导入与 `__all__` —— **漏掉会导致 `create_all` 建表不全**。

- [ ] **Step 3: 跑测试确认通过**

Run: `.venv/bin/python -m pytest tests/test_reserved_models.py -v`

Expected: PASS

- [ ] **Step 4: 在 Task 4 的快照结构里补上占位字段**

Task 4 写 `src/ontology/snapshot.py` 时，`OntologySnapshot` 必须包含：

```python
class OntologySnapshot(BaseModel):
    entity_types: list[EntityTypeSnapshot] = Field(default_factory=list)
    link_types: list[LinkTypeSnapshot] = Field(default_factory=list)
    action_types: list[ActionTypeSnapshot] = Field(default_factory=list)   # 切片 1 恒为空
    functions: list[FunctionSnapshot] = Field(default_factory=list)        # 切片 1 恒为空
```

`OntologyStore` 的 `save_snapshot` / `get_snapshot` 必须**无损往返**这两个列表。切片 1 里它们恒为空列表，但契约先定好。

diff 引擎（Task 9）比较这两个列表时，因为恒为空，行为是「永远无差异」—— 这一点要有测试守住。

**⚠ 2026-09-28 追加：`ActionType.effects[]` 里嵌套的写绑定也必须占位。**

设计规格 §5 定义回写的最小单位是**写绑定（WriteBinding）**，它嵌套在 `ActionType.effects[]` 内部。若切片 1 不把它纳入快照契约，切片 2 引入时必须回头改 `ActionTypeSnapshot` —— 与本 Task 本身是同一类返工。

因此本 Task 追加：

```python
class WriteBindingSnapshot(BaseModel):
    """一次字段级写操作的完整描述。切片 1 恒为空 —— 没有任何写回行为。"""
    system: str                        # 哪个系统
    table: str                         # 哪张表（仅参考，写回走 connector）
    field: str                         # 哪个字段
    key_expr: dict = Field(default_factory=dict)     # 怎么用本体实例定位到那一行
    value_map: dict[str, str] = Field(default_factory=dict)  # 本体值 → 该系统编码
    supported_values: list[str] = Field(default_factory=list) # 哪些值允许落（"维修中"→IoT 时为空）
    conflict_field: str | None = None                  # 乐观锁字段名
    evidence: list[dict] = Field(default_factory=list)


class ActionTypeSnapshot(BaseModel):
    key: str
    target_entity_type: str
    parameters: list[dict] = Field(default_factory=list)
    preconditions: list[dict] = Field(default_factory=list)
    effects: list[WriteBindingSnapshot] = Field(default_factory=list)   # ← 切片 1 恒为空
    permission: dict = Field(default_factory=dict)
    audit_policy: dict = Field(default_factory=dict)
    evidence: list[dict] = Field(default_factory=list)
```

> **三点设计约束要跟着占位一起进快照**（否则切片 2 改结构）：
>
> 1. **哪些系统被写必须显式列出** —— `effects[]` 是一份显式清单，**不做按类型自动推导所有绑定**。自动推导会让「加个字段」静默变成「多写一个客户系统」。
> 2. **`supported_values` 必须能表达「不支持」** —— 设备状态在 IoT 上只有 `0`/`1`，`"维修中"` 不在集合内。这是**有意的表达**，不是遗漏。
> 3. **`conflict_field` 是乐观锁的落点** —— 条件更新 `WHERE row_version = <读到的值>`，影响 0 行即拒绝。
>
> `evidence` 挂在每条写绑定上，回答「**凭什么认为这个系统有资格被写**」—— 这是客户 IT 授权时的第一份清单。

- [ ] **Step 5: Commit**

```bash
git add src/ontology/models src/ontology/snapshot.py tests/test_reserved_models.py
git commit -m "feat: reserve ActionType and Function in the kernel for slice 2"
```

---

## Task 3.6: 类的结构 —— 层级、互斥、别名

> **2026-09-28 新增。设计规格 §4.6 的要求，与 Task 3.5 同属「留全结构」类改动，不可省略。**

此前 `EntityType` 是**扁平**的，无法表达「子类」与「互斥」。这三个字段都不能后补：

| 字段 | 漏了会怎样 |
| --- | --- |
| `parent` | 客户说「设备分机器人和手工的」时只能拆成平级类型 + Link，**语义是错的**（部件 ≠ 关系）。且后续补齐要动 diff 引擎（层级变更是「重挂子树」而非普通修改）与双轨视图 |
| `disjoint_with` | 类型层面的互斥无处安放。后续补齐要动校验与导出 |
| `aliases` | 客户叫法与数据库字段名无法对齐，抽取质量下降；且 SKOS 导出失去数据源 |

**三者的取舍（设计规格 §4.6 已定，本 Task 照做）：**

- `parent` 是 **单继承**（一个字段），**必须做环检测** —— 环会让遍历查询不终止
- `parent` **只继承「是什么」，不自动继承「有哪些关系」** —— 子类自动获得父类全部 LinkType 会让本体规模爆炸，且审核时看不出边界
- `disjoint_with` 在切片 1 **只是审核提示 + 写入校验 + 导出**，**不做推理**

**Files:**
- Modify: `src/ontology/models/ontology.py`（`EntityType` 加三个字段）
- Modify: `src/ontology/models/__init__.py`
- Create: `src/ontology/hierarchy.py`（环检测与祖先/后代计算，纯函数）
- Create: `tests/test_hierarchy.py`
- Create: `tests/test_class_structure.py`
- Modify: `src/ontology/snapshot.py`（Task 4 建快照时并入）

- [ ] **Step 1: 写失败的测试**

Create `tests/test_class_structure.py`:

```python
import pytest
from sqlalchemy.orm import Session

from ontology.models.ontology import EntityType, OntologyVersion
from ontology.models.project import Project


@pytest.fixture
def version_id(pg_engine) -> str:
    with Session(pg_engine) as session:
        project = Project(name="acme")
        session.add(project)
        session.commit()
        version = OntologyVersion(project_id=project.id, seq=1)
        session.add(version)
        session.commit()
        return version.id


def test_entity_type_persists_parent_aliases_and_disjoint(pg_engine, version_id):
    with Session(pg_engine) as session:
        device = EntityType(
            version_id=version_id,
            name="设备",
            aliases=["机台", "机器"],
            disjoint_with=["容器设备"],
        )
        session.add(device)
        session.commit()
        assert device.aliases == ["机台", "机器"]

        robot = EntityType(version_id=version_id, name="机器人设备", parent_id=device.id)
        session.add(robot)
        session.commit()
        assert robot.parent_id == device.id


def test_aliases_default_to_empty(pg_engine, version_id):
    with Session(pg_engine) as session:
        entity = EntityType(version_id=version_id, name="设备")
        session.add(entity)
        session.commit()
        assert entity.aliases == []
        assert entity.disjoint_with == []
        assert entity.parent_id is None


def test_child_cannot_be_its_own_parent(pg_engine, version_id):
    with Session(pg_engine) as session:
        device = EntityType(version_id=version_id, name="设备")
        session.add(device)
        session.commit()
        device.parent_id = device.id
        session.commit()
        # 环检测在领域层做（hierarchy.py），这里守住「写入不会静默产生环」
        from ontology.hierarchy import would_create_cycle

        assert would_create_cycle(
            types={"设备": {"parent": "设备"}},
            candidate="设备",
            new_parent="设备",
        )


def test_same_name_allowed_in_different_versions(pg_engine, version_id):
    """层级是版本内结构，跨版本互不影响。"""
    with Session(pg_engine) as session:
        project_id = session.get(OntologyVersion, version_id).project_id
        v2 = OntologyVersion(project_id=project_id, seq=2)
        session.add(v2)
        session.commit()
        session.add_all([
            EntityType(version_id=version_id, name="设备"),
            EntityType(version_id=v2.id, name="设备"),
        ])
        session.commit()
```

Create `tests/test_hierarchy.py`:

```python
from ontology.hierarchy import ancestors_of, descendants_of, validate_hierarchy


def test_ancestors_walks_up():
    types = {
        "资产": None,
        "设备": "资产",
        "机器人设备": "设备",
    }
    assert ancestors_of("机器人设备", types) == ["设备", "资产"]


def test_descendants_walks_down():
    types = {
        "资产": None,
        "设备": "资产",
        "机器人设备": "设备",
        "手工设备": "设备",
    }
    assert set(descendants_of("设备", types)) == {"机器人设备", "手工设备"}


def test_ancestors_exclude_self():
    """祖先不含自身 —— 与 descendants_of 语义一致，也保证 would_create_cycle 正确。"""
    assert ancestors_of("设备", {"设备": "资产", "资产": None}) == ["资产"]
    assert ancestors_of("资产", {"设备": "资产", "资产": None}) == []


def test_ancestors_stops_on_cycle():
    """防御：环存在时不能无限循环。"""
    assert ancestors_of("A", {"A": "B", "B": "A"}) == ["B"]


def test_validate_rejects_cycle():
    types = {"A": "B", "B": "C", "C": "A"}
    problems = validate_hierarchy(types)
    assert problems, "环必须被检出"


def test_validate_accepts_tree():
    assert validate_hierarchy({"资产": None, "设备": "资产"}) == []


def test_validate_rejects_unknown_parent():
    problems = validate_hierarchy({"设备": "不存在的类型"})
    assert any("不存在的类型" in p for p in problems)
```

- [ ] **Step 2: 实现领域层纯函数**

Create `src/ontology/hierarchy.py`:

```python
"""类层级的纯函数：祖先、后代、环检测、悬空父引用检测。

不 import ORM —— 接收 {name: parent_name | None} 的普通字典，输出普通列表。
"""

from collections.abc import Mapping


def ancestors_of(name: str, types: Mapping[str, str | None]) -> list[str]:
    """从自身向上走到根。返回顺序是「离自己最近的祖先在前」。

    对环做防御：最多访问 len(types) 步，避免无限循环。
    """
    result: list[str] = []
    seen = {name}
    current = types.get(name)
    for _ in range(len(types)):
        if current is None or current not in types:
            break
        if current in seen:
            break
        result.append(current)
        seen.add(current)
        current = types.get(current)
    return result


def descendants_of(name: str, types: Mapping[str, str | None]) -> list[str]:
    """返回所有以 name 为祖先的类型（不含自身）。"""
    children: dict[str, list[str]] = {}
    for child, parent in types.items():
        if parent is not None:
            children.setdefault(parent, []).append(child)

    result: list[str] = []
    stack = list(children.get(name, []))
    seen: set[str] = set()
    while stack:
        current = stack.pop()
        if current in seen:
            continue
        seen.add(current)
        result.append(current)
        stack.extend(children.get(current, []))
    return result


def validate_hierarchy(types: Mapping[str, str | None]) -> list[str]:
    """返回问题清单，空列表表示通过。

    两类问题：
    1. 悬空父引用 —— parent 指向不存在的类型
    2. 环 —— 沿父链走回自己
    """
    problems: list[str] = []

    for name, parent in types.items():
        if parent is not None and parent not in types:
            problems.append(f"{name} 的父类型 {parent} 不存在")

    for name in types:
        chain = [name]
        current = types.get(name)
        while current is not None and current in types:
            if current in chain:
                problems.append(f"层级存在环：{' -> '.join(chain + [current])}")
                break
            chain.append(current)
            current = types.get(current)
    return problems


def would_create_cycle(
    types: Mapping[str, str | None],
    candidate: str,
    new_parent: str | None,
) -> bool:
    """把 candidate 的父改成 new_parent 会不会形成环。"""
    if new_parent is None:
        return False
    if new_parent == candidate:
        return True
    return candidate in ancestors_of(new_parent, types)
```

- [ ] **Step 3: 给 ORM 加字段**

Modify `EntityType` in `src/ontology/models/ontology.py` — 在现有字段之后追加这三行：

```python
    parent_id: Mapped[uuid.UUID | None] = mapped_column(
        Uuid, ForeignKey("entity_type.id", ondelete="SET NULL"), index=True
    )
    aliases: Mapped[list[str]] = mapped_column(JSONB, nullable=False, default=list)
    disjoint_with: Mapped[list[str]] = mapped_column(JSONB, nullable=False, default=list)
```

> 上面是**片段**，直接追加进 `class EntityType` 内部（缩进层级与 `ordinal` 相同）。
>
> `ondelete="SET NULL"` 而非 `CASCADE` —— 删除父类型不应该连带删掉子类型，子类型应当被"提根"而不是消失。**这是有意的选择。**

- [ ] **Step 4: 跑测试确认通过**

Run: `.venv/bin/python -m pytest tests/test_class_structure.py tests/test_hierarchy.py -v`

Expected: PASS

- [ ] **Step 5: 在 Task 4 的快照里带上这三个字段**

Task 4 写 `src/ontology/snapshot.py` 时，`EntityTypeSnapshot` 必须包含：

```python
class EntityTypeSnapshot(BaseModel):
    name: str
    description: str | None = None
    aliases: list[str] = Field(default_factory=list)
    parent: str | None = None            # 父类型的 name，不是 id —— 快照要可读且可 diff
    disjoint_with: list[str] = Field(default_factory=list)
    primary_key: list[str] = Field(default_factory=list)
    properties: list[PropertySnapshot] = Field(default_factory=list)
    evidence: list[dict] = Field(default_factory=list)
```

> **快照里存 `parent` 的 `name` 而不是 `id`** —— 因为快照是给人和 diff 看的，存名字才能直接看出「这个类型的父从机器人设备变成了设备」。

- [ ] **Step 6: diff 引擎要能识别「层级变更」**

Task 9 的 `diff_snapshots` 比较 `parent` 字段时，产出的 diff 条目必须**带 `kind="hierarchy"` 标记**，理由：

- 改一个类型的 `parent`，**它的所有后代在语义上都被牵连**
- 这不是一次普通字段修改，而是一次**影响面变更**
- 因此它**必须走提案通道 + 双轨审核**（设计规格 §4.3.1），不能静默生效

`diff_snapshots` 中对应的一段（片段）：

```python
# parent 字段变更 → 层级变更，附带受影响的后代
diff_item(
    kind="hierarchy",
    target="机器人设备",
    before={"parent": "设备"},
    after={"parent": "数控设备"},
    impacted=descendants_of("机器人设备", types),   # 受影响的后代，不含自身
)
```

> `impacted` 让 FDE 在审核界面上直接看到「这一改会波及哪些类型」—— 这正是我们讨论过的「规则引擎先算影响面、人工再决定」的手工版本。切片 1 不需要规则引擎，一个递归函数就够。

- [ ] **Step 7: Commit**

```bash
git add src/ontology/models src/ontology/hierarchy.py tests/test_class_structure.py tests/test_hierarchy.py
git commit -m "feat: add class hierarchy, disjointness and aliases to EntityType"
```

---

## Task 4: 快照结构与 Store 抽象

`ontology-store` 是存储抽象（设计文档 §4.4），GraphDB 只是它的实现之一。这一层的关键是**中立性**：调用方拿到的是 Pydantic 快照，不是 ORM 对象。这样换存储实现不会波及领域代码。

**Files:**
- Create: `src/ontology/snapshot.py`
- Create: `src/ontology/store/__init__.py`
- Create: `src/ontology/store/base.py`
- Create: `src/ontology/store/memory.py`
- Create: `tests/test_store_contract.py`

- [ ] **Step 1: 写快照结构与存储接口**

这些是数据结构定义，没有行为可测，直接写。

Create `src/ontology/snapshot.py`:

```python
"""中立快照结构：存储层与领域层之间唯一的数据契约。

领域代码（diff、审核、应用提案）只认这里的模型，不 import ORM。
"""

from pydantic import BaseModel, Field


class PropertySnapshot(BaseModel):
    name: str
    data_type: str
    nullable: bool = True
    source_ref: dict = Field(default_factory=dict)
    evidence: list[dict] = Field(default_factory=list)


class EntityTypeSnapshot(BaseModel):
    name: str
    description: str | None = None
    primary_key: list[str] = Field(default_factory=list)
    properties: list[PropertySnapshot] = Field(default_factory=list)
    evidence: list[dict] = Field(default_factory=list)


class LinkTypeSnapshot(BaseModel):
    """关系快照。两端用实体类型名引用，不用 id —— 名字才是跨版本稳定的标识。"""

    name: str
    from_entity: str
    to_entity: str
    cardinality: str
    evidence: list[dict] = Field(default_factory=list)


class OntologySnapshot(BaseModel):
    entity_types: list[EntityTypeSnapshot] = Field(default_factory=list)
    link_types: list[LinkTypeSnapshot] = Field(default_factory=list)

    def entity_type(self, name: str) -> EntityTypeSnapshot | None:
        for et in self.entity_types:
            if et.name == name:
                return et
        return None
```

Create `src/ontology/store/base.py`:

```python
from abc import ABC, abstractmethod

from pydantic import BaseModel

from ontology.snapshot import OntologySnapshot


class VersionInfo(BaseModel):
    id: str
    project_id: str
    seq: int
    status: str
    parent_version_id: str | None = None


class VersionFrozenError(RuntimeError):
    """试图写入已冻结的版本。"""


class OntologyStore(ABC):
    """本体存储抽象。实现可以是 PostgreSQL、图数据库或内存。"""

    @abstractmethod
    def create_project(self, name: str, description: str | None = None) -> str:
        """新建项目并返回其 id。

        项目是本体、提案、审核记录的归属边界（§7.2）。抽象层必须管这件事 ——
        否则调用方得自己拼 UUID，而 PostgreSQL 实现的外键约束会直接拒绝它们。
        """

    @abstractmethod
    def create_version(
        self, project_id: str, parent_version_id: str | None = None, note: str | None = None
    ) -> VersionInfo:
        """新建一个 draft 版本，seq 取该项目当前最大值 +1。"""

    @abstractmethod
    def get_version(self, version_id: str) -> VersionInfo: ...

    @abstractmethod
    def list_versions(self, project_id: str) -> list[VersionInfo]:
        """按 seq 升序。"""

    @abstractmethod
    def latest_version(self, project_id: str) -> VersionInfo | None: ...

    @abstractmethod
    def get_snapshot(self, version_id: str) -> OntologySnapshot: ...

    @abstractmethod
    def save_snapshot(self, version_id: str, snapshot: OntologySnapshot) -> None:
        """全量替换该版本的内容。版本已冻结时抛 VersionFrozenError。"""

    @abstractmethod
    def freeze_version(self, version_id: str) -> None: ...
```

Create `src/ontology/store/__init__.py`:

```python
from ontology.store.base import (
    OntologyStore,
    VersionFrozenError,
    VersionInfo,
)
from ontology.store.memory import InMemoryOntologyStore

__all__ = [
    "InMemoryOntologyStore",
    "OntologyStore",
    "VersionFrozenError",
    "VersionInfo",
]
```

Create `src/ontology/store/memory.py`:

```python
from uuid import uuid4

from ontology.snapshot import OntologySnapshot
from ontology.store.base import OntologyStore, VersionFrozenError, VersionInfo


class InMemoryOntologyStore(OntologyStore):
    """内存实现。仅用于测试与演示，进程退出即丢失。"""

    def __init__(self) -> None:
        self._versions: dict[str, VersionInfo] = {}
        self._snapshots: dict[str, OntologySnapshot] = {}
        self._next_seq: dict[str, int] = {}
        self._projects: dict[str, dict] = {}
        self._counter = 0

    def _new_id(self) -> str:
        self._counter += 1
        return f"v{self._counter}"

    def create_project(self, name: str, description: str | None = None) -> str:
        project_id = str(uuid4())
        self._projects[project_id] = {"name": name, "description": description}
        return project_id

    def create_version(self, project_id, parent_version_id=None, note=None) -> VersionInfo:
        seq = self._next_seq.get(project_id, 0) + 1
        self._next_seq[project_id] = seq
        version = VersionInfo(
            id=self._new_id(),
            project_id=project_id,
            seq=seq,
            status="draft",
            parent_version_id=parent_version_id,
        )
        self._versions[version.id] = version
        self._snapshots[version.id] = OntologySnapshot()
        return version

    def get_version(self, version_id: str) -> VersionInfo:
        return self._versions[version_id]

    def list_versions(self, project_id: str) -> list[VersionInfo]:
        versions = [v for v in self._versions.values() if v.project_id == project_id]
        return sorted(versions, key=lambda v: v.seq)

    def latest_version(self, project_id: str) -> VersionInfo | None:
        versions = self.list_versions(project_id)
        return versions[-1] if versions else None

    def get_snapshot(self, version_id: str) -> OntologySnapshot:
        return self._snapshots[version_id].model_copy(deep=True)

    def save_snapshot(self, version_id: str, snapshot: OntologySnapshot) -> None:
        if self._versions[version_id].status == "frozen":
            raise VersionFrozenError(f"版本 {version_id} 已冻结，不可写入")
        self._snapshots[version_id] = snapshot.model_copy(deep=True)

    def freeze_version(self, version_id: str) -> None:
        current = self._versions[version_id]
        self._versions[version_id] = current.model_copy(update={"status": "frozen"})
```

- [ ] **Step 2: 写契约测试（先只参数化内存实现）**

契约测试的意义：Task 5 的 PostgreSQL 实现必须跑通同一套断言，否则两个实现会行为漂移。

Create `tests/test_store_contract.py`:

```python
"""存储契约测试。

内存实现与 PostgreSQL 实现共用同一套断言 —— 这是 §4.4「存储可替换」的机制保证，
不是文档里的一句承诺。Task 5 会把 "pg" 加入 params。
"""

import pytest

from ontology.snapshot import EntityTypeSnapshot, LinkTypeSnapshot, OntologySnapshot
from ontology.store import InMemoryOntologyStore, VersionFrozenError


@pytest.fixture(params=["memory"])
def store(request):
    # 不声明 pg_engine 依赖：否则没有测试库时，连 memory 参数下的断言
    # 也会被一并 skip，内存实现就永远得不到验证。
    if request.param == "memory":
        return InMemoryOntologyStore()
    raise AssertionError(f"未实现的 store 参数: {request.param}")


@pytest.fixture
def project_id(store) -> str:
    """走 store 自己的建项目入口，两种实现面对同一份真实输入。

    不要写硬编码字符串：PostgreSQL 实现要拿它查 UUID 且受外键约束，
    传入 "project-1" 会直接抛 ValueError。
    """
    return store.create_project("契约测试项目")


def test_create_project_returns_usable_id(store):
    project_id = store.create_project("独立项目")
    assert store.create_version(project_id).project_id == project_id


def _sample_snapshot() -> OntologySnapshot:
    return OntologySnapshot(
        entity_types=[
            EntityTypeSnapshot(
                name="设备",
                description="生产设备",
                primary_key=["dev_id"],
                properties=[
                    {"name": "sn", "data_type": "text", "source_ref": {"table": "t_dev"}}
                ],
                evidence=[{"table": "t_dev", "columns": ["dev_id", "sn"]}],
            ),
            EntityTypeSnapshot(name="工单", primary_key=["wo_id"]),
        ],
        link_types=[
            LinkTypeSnapshot(
                name="设备-工单",
                from_entity="设备",
                to_entity="工单",
                cardinality="one_to_many",
            )
        ],
    )


def test_create_version_starts_at_seq_one(store, project_id):
    version = store.create_version(project_id)
    assert version.seq == 1
    assert version.status == "draft"
    assert version.project_id == project_id


def test_create_version_increments_seq(store, project_id):
    store.create_version(project_id)
    second = store.create_version(project_id)
    assert second.seq == 2


def test_seq_is_scoped_to_project(store):
    first_project = store.create_project("项目A")
    second_project = store.create_project("项目B")
    store.create_version(first_project)
    assert store.create_version(second_project).seq == 1


def test_latest_version_returns_highest_seq(store, project_id):
    store.create_version(project_id)
    second = store.create_version(project_id)
    assert store.latest_version(project_id).id == second.id


def test_latest_version_is_none_for_empty_project(store, project_id):
    assert store.latest_version(project_id) is None


def test_list_versions_is_ordered_by_seq(store, project_id):
    first = store.create_version(project_id)
    second = store.create_version(project_id)
    assert [v.id for v in store.list_versions(project_id)] == [first.id, second.id]


def test_new_version_has_empty_snapshot(store, project_id):
    version = store.create_version(project_id)
    snapshot = store.get_snapshot(version.id)
    assert snapshot.entity_types == []
    assert snapshot.link_types == []


def test_snapshot_roundtrip(store, project_id):
    version = store.create_version(project_id)
    store.save_snapshot(version.id, _sample_snapshot())

    loaded = store.get_snapshot(version.id)
    assert [et.name for et in loaded.entity_types] == ["设备", "工单"]
    assert loaded.entity_type("设备").primary_key == ["dev_id"]
    assert loaded.entity_type("设备").properties[0].name == "sn"
    assert loaded.entity_type("设备").properties[0].source_ref == {"table": "t_dev"}
    assert loaded.link_types[0].from_entity == "设备"
    assert loaded.link_types[0].cardinality == "one_to_many"


def test_save_snapshot_replaces_previous_content(store, project_id):
    version = store.create_version(project_id)
    store.save_snapshot(version.id, _sample_snapshot())
    store.save_snapshot(
        version.id, OntologySnapshot(entity_types=[EntityTypeSnapshot(name="仅此一个")])
    )

    loaded = store.get_snapshot(version.id)
    assert [et.name for et in loaded.entity_types] == ["仅此一个"]
    assert loaded.link_types == []


def test_get_snapshot_returns_a_copy(store, project_id):
    """调用方改动返回值不应污染存储 —— 否则 diff 与审核会产生幽灵变更。"""
    version = store.create_version(project_id)
    store.save_snapshot(version.id, _sample_snapshot())

    loaded = store.get_snapshot(version.id)
    loaded.entity_types.append(EntityTypeSnapshot(name="偷偷加的"))
    assert [et.name for et in store.get_snapshot(version.id).entity_types] == ["设备", "工单"]


def test_frozen_version_rejects_writes(store, project_id):
    version = store.create_version(project_id)
    store.freeze_version(version.id)
    assert store.get_version(version.id).status == "frozen"
    with pytest.raises(VersionFrozenError):
        store.save_snapshot(version.id, _sample_snapshot())


def test_frozen_version_is_immutable_after_freeze(store, project_id):
    version = store.create_version(project_id)
    store.save_snapshot(version.id, _sample_snapshot())
    store.freeze_version(version.id)
    assert [et.name for et in store.get_snapshot(version.id).entity_types] == ["设备", "工单"]
```

- [ ] **Step 3: 跑测试确认通过**

Run: `.venv/bin/python -m pytest tests/test_store_contract.py -v`

Expected: PASS（13 passed）

- [ ] **Step 4: Commit**

```bash
git add src/ontology/snapshot.py src/ontology/store tests/test_store_contract.py
git commit -m "feat: add ontology store abstraction with in-memory implementation"
```

---

## Task 5: PostgreSQL 实现

同一套契约测试跑通第二个实现。若某些断言在 PG 下失败，说明抽象有泄漏 —— 修抽象，不要放宽测试。

**Files:**
- Create: `src/ontology/store/pg.py`
- Modify: `src/ontology/store/__init__.py`
- Modify: `tests/test_store_contract.py`

- [ ] **Step 1: 把 pg 加入契约测试参数，确认失败**

Modify `tests/test_store_contract.py` — 把 `store` fixture 改成：

```python
@pytest.fixture(params=["memory", "pg"])
def store(request):
    if request.param == "memory":
        return InMemoryOntologyStore()
    # 惰性获取：memory 参数下不查 PG。若把它写成 fixture 形参，
    # 没配测试库时 pg_engine 的 skip 会把两个实现一起跳过。
    engine = request.getfixturevalue("pg_engine")
    return PostgresOntologyStore(engine)
```

同时把 import 行改成：

```python
from ontology.store import (
    InMemoryOntologyStore,
    PostgresOntologyStore,
    VersionFrozenError,
)
```

Run: `.venv/bin/python -m pytest tests/test_store_contract.py -v`

Expected: FAIL — `ImportError: cannot import name 'PostgresOntologyStore'`

- [ ] **Step 2: 实现 PostgreSQL 存储**

Create `src/ontology/store/pg.py`:

```python
"""PostgreSQL 实现。

设计文档 §4.4：企业本体查询深度通常 <= 3 跳，PG 完全够用，且客户环境里本来就有 PG。
GraphDB 实现将来接在同一个抽象下，不改动上层。
"""

import uuid

from sqlalchemy import Engine, delete, select
from sqlalchemy.orm import Session

from ontology.models.base import utcnow
from ontology.models.ontology import EntityProperty, EntityType, LinkType, OntologyVersion
from ontology.models.project import Project
from ontology.snapshot import EntityTypeSnapshot, LinkTypeSnapshot, OntologySnapshot, PropertySnapshot
from ontology.store.base import OntologyStore, VersionFrozenError, VersionInfo


def _as_uuid(value: str) -> uuid.UUID:
    return uuid.UUID(value)


class PostgresOntologyStore(OntologyStore):
    def __init__(self, engine: Engine) -> None:
        self._engine = engine

    def create_project(self, name: str, description: str | None = None) -> str:
        with Session(self._engine) as session:
            project = Project(name=name, description=description)
            session.add(project)
            session.commit()
            return str(project.id)

    def create_version(self, project_id, parent_version_id=None, note=None) -> VersionInfo:
        with Session(self._engine) as session:
            latest = session.execute(
                select(OntologyVersion.seq)
                .where(OntologyVersion.project_id == _as_uuid(project_id))
                .order_by(OntologyVersion.seq.desc())
                .limit(1)
            ).scalar_one_or_none()
            version = OntologyVersion(
                project_id=_as_uuid(project_id),
                seq=(latest or 0) + 1,
                status="draft",
                parent_version_id=_as_uuid(parent_version_id) if parent_version_id else None,
                note=note,
            )
            session.add(version)
            session.commit()
            return _to_info(version)

    def get_version(self, version_id: str) -> VersionInfo:
        with Session(self._engine) as session:
            version = session.get(OntologyVersion, _as_uuid(version_id))
            if version is None:
                raise KeyError(f"版本不存在: {version_id}")
            return _to_info(version)

    def list_versions(self, project_id: str) -> list[VersionInfo]:
        with Session(self._engine) as session:
            rows = session.execute(
                select(OntologyVersion)
                .where(OntologyVersion.project_id == _as_uuid(project_id))
                .order_by(OntologyVersion.seq)
            ).scalars()
            return [_to_info(v) for v in rows]

    def latest_version(self, project_id: str) -> VersionInfo | None:
        versions = self.list_versions(project_id)
        return versions[-1] if versions else None

    def get_snapshot(self, version_id: str) -> OntologySnapshot:
        with Session(self._engine) as session:
            vid = _as_uuid(version_id)
            entities = session.execute(
                select(EntityType).where(EntityType.version_id == vid).order_by(EntityType.ordinal)
            ).scalars().all()
            links = session.execute(
                select(LinkType).where(LinkType.version_id == vid).order_by(LinkType.ordinal)
            ).scalars().all()

            name_by_entity_id = {e.id: e.name for e in entities}

            entity_snapshots = []
            for entity in entities:
                props = session.execute(
                    select(EntityProperty)
                    .where(EntityProperty.entity_type_id == entity.id)
                    .order_by(EntityProperty.ordinal)
                ).scalars().all()
                entity_snapshots.append(
                    EntityTypeSnapshot(
                        name=entity.name,
                        description=entity.description,
                        primary_key=list(entity.pk_columns or []),
                        properties=[
                            PropertySnapshot(
                                name=p.name,
                                data_type=p.data_type,
                                nullable=p.nullable,
                                source_ref=dict(p.source_ref or {}),
                                evidence=list(p.evidence or []),
                            )
                            for p in props
                        ],
                        evidence=list(entity.evidence or []),
                    )
                )

            link_snapshots = [
                LinkTypeSnapshot(
                    name=link.name,
                    from_entity=name_by_entity_id[link.from_entity_type_id],
                    to_entity=name_by_entity_id[link.to_entity_type_id],
                    cardinality=link.cardinality,
                    evidence=list(link.evidence or []),
                )
                for link in links
            ]

            return OntologySnapshot(entity_types=entity_snapshots, link_types=link_snapshots)

    def save_snapshot(self, version_id: str, snapshot: OntologySnapshot) -> None:
        with Session(self._engine) as session:
            vid = _as_uuid(version_id)
            version = session.get(OntologyVersion, vid)
            if version is None:
                raise KeyError(f"版本不存在: {version_id}")
            if version.status == "frozen":
                raise VersionFrozenError(f"版本 {version_id} 已冻结，不可写入")

            # 全量替换。先在库层面清空该版本的内容，再按快照重建 ——
            # 简单可靠优先，本体规模（几十个类型）远未到需要增量 diff 更新的量级。
            # 直接发 DELETE 而不走 session.delete(entity)：模型没定义 ORM
            # relationship，Session 并不知道有子行依赖被删对象，交给数据库的
            # ON DELETE CASCADE 更明确。
            session.execute(
                delete(EntityProperty).where(
                    EntityProperty.entity_type_id.in_(
                        select(EntityType.id).where(EntityType.version_id == vid)
                    )
                )
            )
            session.execute(delete(LinkType).where(LinkType.version_id == vid))
            session.execute(delete(EntityType).where(EntityType.version_id == vid))
            session.flush()

            entity_ids: dict[str, uuid.UUID] = {}
            for index, et in enumerate(snapshot.entity_types):
                entity = EntityType(
                    version_id=vid,
                    name=et.name,
                    description=et.description,
                    pk_columns=list(et.primary_key),
                    evidence=list(et.evidence),
                    ordinal=index,
                )
                session.add(entity)
                session.flush()
                entity_ids[et.name] = entity.id
                for prop_index, prop in enumerate(et.properties):
                    session.add(
                        EntityProperty(
                            entity_type_id=entity.id,
                            name=prop.name,
                            data_type=prop.data_type,
                            nullable=prop.nullable,
                            source_ref=dict(prop.source_ref),
                            evidence=list(prop.evidence),
                            ordinal=prop_index,
                        )
                    )

            for index, link in enumerate(snapshot.link_types):
                if link.from_entity not in entity_ids or link.to_entity not in entity_ids:
                    raise ValueError(
                        f"关系 {link.name} 引用了不存在的实体类型:"
                        f" {link.from_entity} -> {link.to_entity}"
                    )
                session.add(
                    LinkType(
                        version_id=vid,
                        name=link.name,
                        from_entity_type_id=entity_ids[link.from_entity],
                        to_entity_type_id=entity_ids[link.to_entity],
                        cardinality=link.cardinality,
                        evidence=list(link.evidence),
                        ordinal=index,
                    )
                )

            session.commit()

    def freeze_version(self, version_id: str) -> None:
        with Session(self._engine) as session:
            vid = _as_uuid(version_id)
            version = session.get(OntologyVersion, vid)
            if version is None:
                raise KeyError(f"版本不存在: {version_id}")
            version.status = "frozen"
            version.frozen_at = utcnow()
            session.commit()


def _to_info(version: OntologyVersion) -> VersionInfo:
    return VersionInfo(
        id=str(version.id),
        project_id=str(version.project_id),
        seq=version.seq,
        status=version.status,
        parent_version_id=str(version.parent_version_id) if version.parent_version_id else None,
    )
```

- [ ] **Step 3: 导出实现**

Modify `src/ontology/store/__init__.py` — 改成：

```python
from ontology.store.base import OntologyStore, VersionFrozenError, VersionInfo
from ontology.store.memory import InMemoryOntologyStore
from ontology.store.pg import PostgresOntologyStore

__all__ = [
    "InMemoryOntologyStore",
    "OntologyStore",
    "PostgresOntologyStore",
    "VersionFrozenError",
    "VersionInfo",
]
```

- [ ] **Step 4: 跑契约测试确认两个实现都通过**

Run: `.venv/bin/python -m pytest tests/test_store_contract.py -v`

Expected: PASS（26 passed —— 13 个契约测试 × 2 个实现）

若出现 `ERROR: fixture 'pg_engine' not found`，检查 `tests/conftest.py` 是否已建立。

- [ ] **Step 5: Commit**

```bash
git add src/ontology/store tests/test_store_contract.py
git commit -m "feat: add postgresql ontology store implementation"
```

---

## Task 6: 提案与审核记录模型

设计文档 §2.2：agent 是提议者不是决定者。提案是 agent 输出进入系统的唯一形式，审核记录是它落地的唯一途径。

本条只做数据模型，审核逻辑在 Task 7。

**Files:**
- Create: `src/ontology/models/proposal.py`
- Modify: `src/ontology/models/__init__.py`
- Create: `tests/test_proposal_models.py`

- [ ] **Step 1: 写失败的测试**

Create `tests/test_proposal_models.py`:

```python
import pytest
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session

from ontology.models.project import Project
from ontology.models.proposal import Proposal, ProposalItem, ReviewRecord


@pytest.fixture
def project_id(pg_engine) -> str:
    with Session(pg_engine) as session:
        project = Project(name="acme")
        session.add(project)
        session.commit()
        return project.id


@pytest.fixture
def proposal_id(pg_engine, project_id) -> str:
    with Session(pg_engine) as session:
        proposal = Proposal(project_id=project_id, source="extraction")
        session.add(proposal)
        session.commit()
        return proposal.id


def test_proposal_defaults(pg_engine, proposal_id):
    with Session(pg_engine) as session:
        proposal = session.get(Proposal, proposal_id)
        assert proposal.status == "draft"
        assert proposal.base_version_id is None


def test_proposal_status_rejects_unknown_value(pg_engine, project_id):
    with Session(pg_engine) as session:
        session.add(Proposal(project_id=project_id, source="extraction", status="whatever"))
        with pytest.raises(IntegrityError):
            session.commit()


def test_item_defaults_to_pending_on_both_tracks(pg_engine, proposal_id):
    with Session(pg_engine) as session:
        item = ProposalItem(
            proposal_id=proposal_id,
            kind="add_entity_type",
            target_name="设备",
            payload={"name": "设备", "primary_key": ["dev_id"]},
            evidence=[{"table": "t_dev"}],
        )
        session.add(item)
        session.commit()
        assert item.biz_status == "pending"
        assert item.model_status == "pending"


def test_item_kind_rejects_unknown_value(pg_engine, proposal_id):
    with Session(pg_engine) as session:
        session.add(
            ProposalItem(proposal_id=proposal_id, kind="invent_something", target_name="x")
        )
        with pytest.raises(IntegrityError):
            session.commit()


def test_review_record_captures_track_and_decision(pg_engine, proposal_id):
    with Session(pg_engine) as session:
        item = ProposalItem(
            proposal_id=proposal_id, kind="add_entity_type", target_name="设备"
        )
        session.add(item)
        session.commit()

        session.add(
            ReviewRecord(
                item_id=item.id,
                track="biz",
                decision="accept",
                reviewer="zhang@acme",
                comment="这是我们说的设备",
            )
        )
        session.commit()

    with Session(pg_engine) as session:
        records = session.query(ReviewRecord).all()
        assert len(records) == 1
        assert records[0].track == "biz"
        assert records[0].comment == "这是我们说的设备"


def test_review_record_rejects_unknown_track(pg_engine, proposal_id):
    with Session(pg_engine) as session:
        item = ProposalItem(
            proposal_id=proposal_id, kind="add_entity_type", target_name="设备"
        )
        session.add(item)
        session.commit()
        session.add(
            ReviewRecord(item_id=item.id, track="legal", decision="accept", reviewer="x")
        )
        with pytest.raises(IntegrityError):
            session.commit()
```

- [ ] **Step 2: 跑测试确认失败**

Run: `.venv/bin/python -m pytest tests/test_proposal_models.py -v`

Expected: FAIL — `ModuleNotFoundError: No module named 'ontology.models.proposal'`

- [ ] **Step 3: 实现模型**

Create `src/ontology/models/proposal.py`:

```python
import uuid

from sqlalchemy import CheckConstraint, ForeignKey, String, UniqueConstraint, Uuid
from sqlalchemy.dialects.postgresql import JSONB
from sqlalchemy.orm import Mapped, mapped_column

from ontology.models.base import Base, TimestampMixin

PROPOSAL_STATUSES = ("draft", "under_review", "approved", "partial", "rejected", "applied")
PROPOSAL_SOURCES = ("extraction", "manual")
ITEM_KINDS = (
    "add_entity_type",
    "add_property",
    "add_link_type",
    "modify_entity_type",
    "remove_entity_type",
)
TRACK_STATUSES = ("pending", "accepted", "rejected")


class Proposal(Base, TimestampMixin):
    """一次本体变更建议。来源是抽取或人工，产物必须经双轨审核才能生效。"""

    __tablename__ = "proposal"
    __table_args__ = (
        CheckConstraint(
            "status in ('draft','under_review','approved','partial','rejected','applied')",
            name="ck_proposal_status",
        ),
        CheckConstraint("source in ('extraction','manual')", name="ck_proposal_source"),
    )

    id: Mapped[uuid.UUID] = mapped_column(Uuid, primary_key=True, default=uuid.uuid4)
    project_id: Mapped[uuid.UUID] = mapped_column(
        Uuid, ForeignKey("project.id", ondelete="CASCADE"), nullable=False, index=True
    )
    # 提案基于哪个版本产生。为空表示从零构建（首轮抽取）。
    base_version_id: Mapped[uuid.UUID | None] = mapped_column(
        Uuid, ForeignKey("ontology_version.id", ondelete="SET NULL")
    )
    source: Mapped[str] = mapped_column(String(20), nullable=False)
    status: Mapped[str] = mapped_column(String(20), nullable=False, default="draft")
    note: Mapped[str | None] = mapped_column(String(2000))


class ProposalItem(Base, TimestampMixin):
    """提案中的一条变更。两条审核轨各自独立判定 —— 一条不对不应拖垮整份提案。"""

    __tablename__ = "proposal_item"
    __table_args__ = (
        CheckConstraint(
            "kind in ('add_entity_type','add_property','add_link_type',"
            "'modify_entity_type','remove_entity_type')",
            name="ck_item_kind",
        ),
        CheckConstraint(
            "biz_status in ('pending','accepted','rejected')", name="ck_item_biz_status"
        ),
        CheckConstraint(
            "model_status in ('pending','accepted','rejected')", name="ck_item_model_status"
        ),
        UniqueConstraint("proposal_id", "kind", "target_name", name="uq_item_proposal_target"),
    )

    id: Mapped[uuid.UUID] = mapped_column(Uuid, primary_key=True, default=uuid.uuid4)
    proposal_id: Mapped[uuid.UUID] = mapped_column(
        Uuid, ForeignKey("proposal.id", ondelete="CASCADE"), nullable=False, index=True
    )
    kind: Mapped[str] = mapped_column(String(30), nullable=False)
    # 变更指向的对象名。link 类型用 "设备->工单" 形式，保证唯一约束能覆盖。
    target_name: Mapped[str] = mapped_column(String(400), nullable=False)
    payload: Mapped[dict] = mapped_column(JSONB, nullable=False, default=dict)
    evidence: Mapped[list[dict]] = mapped_column(JSONB, nullable=False, default=list)
    biz_status: Mapped[str] = mapped_column(String(20), nullable=False, default="pending")
    model_status: Mapped[str] = mapped_column(String(20), nullable=False, default="pending")


class ReviewRecord(Base, TimestampMixin):
    """一次审核动作。只追加不修改 —— 审核历史是追溯责任链的依据。"""

    __tablename__ = "review_record"
    __table_args__ = (
        CheckConstraint("track in ('biz','model')", name="ck_record_track"),
        CheckConstraint("decision in ('accept','reject')", name="ck_record_decision"),
    )

    id: Mapped[uuid.UUID] = mapped_column(Uuid, primary_key=True, default=uuid.uuid4)
    item_id: Mapped[uuid.UUID] = mapped_column(
        Uuid, ForeignKey("proposal_item.id", ondelete="CASCADE"), nullable=False, index=True
    )
    track: Mapped[str] = mapped_column(String(10), nullable=False)
    decision: Mapped[str] = mapped_column(String(10), nullable=False)
    reviewer: Mapped[str] = mapped_column(String(200), nullable=False)
    comment: Mapped[str | None] = mapped_column(String(2000))
```

- [ ] **Step 4: 注册到 models 包**

Modify `src/ontology/models/__init__.py` — 改成：

```python
from ontology.models.base import Base, TimestampMixin, utcnow
from ontology.models.ontology import EntityProperty, EntityType, LinkType, OntologyVersion
from ontology.models.project import Project
from ontology.models.proposal import Proposal, ProposalItem, ReviewRecord

__all__ = [
    "Base",
    "EntityProperty",
    "EntityType",
    "LinkType",
    "OntologyVersion",
    "Project",
    "Proposal",
    "ProposalItem",
    "ReviewRecord",
    "TimestampMixin",
    "utcnow",
]
```

- [ ] **Step 5: 跑测试确认通过**

Run: `.venv/bin/python -m pytest tests/test_proposal_models.py -v`

Expected: PASS（6 passed）

- [ ] **Step 6: Commit**

```bash
git add src/ontology/models tests/test_proposal_models.py
git commit -m "feat: add proposal and review record models"
```

---

## Task 7: 双轨审核状态机

设计文档 §6.2：客户审语义正确性，FDE 审建模质量。两条轨独立判定，但**关系类变更依赖它两端的实体类型已被接受** —— 否则会产生引用了不存在实体的野指针。

本条是纯逻辑，不碰数据库：接收 item 对象列表，返回新状态。

**Files:**
- Create: `src/ontology/review/__init__.py`
- Create: `src/ontology/review/state.py`
- Create: `tests/test_review_state.py`

- [ ] **Step 1: 写失败的测试**

Create `tests/test_review_state.py`:

```python
import pytest

from ontology.review.state import (
    Decision,
    DependencyNotMet,
    ItemView,
    ProposalStatus,
    Track,
    apply_decision,
    check_dependencies,
    summarize_proposal,
)


def _item(
    kind: str,
    target: str,
    payload: dict | None = None,
    biz: str = "pending",
    model: str = "pending",
) -> ItemView:
    return ItemView(
        id=f"{kind}:{target}",
        kind=kind,
        target_name=target,
        payload=payload or {},
        biz_status=biz,
        model_status=model,
    )


def test_apply_decision_accepted_on_biz_track_only():
    item = _item("add_entity_type", "设备")
    apply_decision(item, Track.BIZ, Decision.ACCEPT)
    assert item.biz_status == "accepted"
    assert item.model_status == "pending"


def test_apply_decision_rejected_on_model_track_only():
    item = _item("add_entity_type", "设备")
    apply_decision(item, Track.MODEL, Decision.REJECT)
    assert item.model_status == "rejected"
    assert item.biz_status == "pending"


def test_both_tracks_accepted_marks_item_accepted():
    item = _item("add_entity_type", "设备")
    apply_decision(item, Track.BIZ, Decision.ACCEPT)
    apply_decision(item, Track.MODEL, Decision.ACCEPT)
    assert item.is_accepted is True


def test_rejected_on_either_track_marks_item_rejected():
    item = _item("add_entity_type", "设备")
    apply_decision(item, Track.BIZ, Decision.ACCEPT)
    apply_decision(item, Track.MODEL, Decision.REJECT)
    assert item.is_rejected is True
    assert item.is_accepted is False


def test_link_without_entity_dependency_is_blocked():
    """关系引用的实体类型不在本提案里、也不在基线快照里 —— 必须拦住。"""
    link = _item(
        "add_link_type",
        "设备->工单",
        {"name": "设备-工单", "from_entity": "设备", "to_entity": "工单"},
    )
    with pytest.raises(DependencyNotMet) as excinfo:
        check_dependencies(link, all_items=[link], baseline_entity_names=[])
    assert "设备" in str(excinfo.value)


def test_link_passes_when_both_ends_declared_in_proposal():
    device = _item("add_entity_type", "设备")
    ticket = _item("add_entity_type", "工单")
    link = _item(
        "add_link_type",
        "设备->工单",
        {"name": "设备-工单", "from_entity": "设备", "to_entity": "工单"},
    )
    check_dependencies(link, all_items=[device, ticket, link], baseline_entity_names=[])


def test_link_passes_when_both_ends_exist_in_baseline():
    link = _item(
        "add_link_type",
        "设备->工单",
        {"name": "设备-工单", "from_entity": "设备", "to_entity": "工单"},
    )
    check_dependencies(link, all_items=[link], baseline_entity_names=["设备", "工单"])


def test_link_acceptance_requires_both_ends_accepted():
    device = _item("add_entity_type", "设备", biz="accepted", model="accepted")
    ticket = _item("add_entity_type", "工单", biz="accepted")  # 建模轨还没过
    link = _item(
        "add_link_type",
        "设备->工单",
        {"name": "设备-工单", "from_entity": "设备", "to_entity": "工单"},
    )
    with pytest.raises(DependencyNotMet) as excinfo:
        check_dependencies(link, all_items=[device, ticket, link], baseline_entity_names=[])
    assert "工单" in str(excinfo.value)


def test_link_acceptance_ok_when_both_ends_fully_accepted():
    device = _item("add_entity_type", "设备", biz="accepted", model="accepted")
    ticket = _item("add_entity_type", "工单", biz="accepted", model="accepted")
    link = _item(
        "add_link_type",
        "设备->工单",
        {"name": "设备-工单", "from_entity": "设备", "to_entity": "工单"},
    )
    check_dependencies(link, all_items=[device, ticket, link], baseline_entity_names=[])


def test_baseline_entity_needs_no_acceptance():
    """基线里已有的实体类型天然满足依赖，不要求本提案再接受一次。"""
    link = _item(
        "add_link_type",
        "设备->工单",
        {"name": "设备-工单", "from_entity": "设备", "to_entity": "工单"},
    )
    check_dependencies(link, all_items=[link], baseline_entity_names=["设备", "工单"])


def test_non_link_items_have_no_dependencies():
    prop = _item("add_property", "设备.sn", {"entity": "设备", "name": "sn"})
    check_dependencies(prop, all_items=[prop], baseline_entity_names=[])


def test_summarize_returns_under_review_while_any_pending():
    items = [
        _item("add_entity_type", "设备", biz="accepted", model="accepted"),
        _item("add_entity_type", "工单"),
    ]
    assert summarize_proposal(items) is ProposalStatus.UNDER_REVIEW


def test_summarize_returns_approved_when_all_accepted():
    items = [
        _item("add_entity_type", "设备", biz="accepted", model="accepted"),
        _item("add_entity_type", "工单", biz="accepted", model="accepted"),
    ]
    assert summarize_proposal(items) is ProposalStatus.APPROVED


def test_summarize_returns_rejected_when_all_rejected():
    items = [
        _item("add_entity_type", "设备", biz="rejected", model="accepted"),
        _item("add_entity_type", "工单", biz="rejected", model="rejected"),
    ]
    assert summarize_proposal(items) is ProposalStatus.REJECTED


def test_summarize_returns_partial_on_mixed_outcome():
    items = [
        _item("add_entity_type", "设备", biz="accepted", model="accepted"),
        _item("add_entity_type", "工单", biz="rejected", model="accepted"),
    ]
    assert summarize_proposal(items) is ProposalStatus.PARTIAL


def test_summarize_empty_proposal_is_under_review():
    assert summarize_proposal([]) is ProposalStatus.UNDER_REVIEW
```

- [ ] **Step 2: 跑测试确认失败**

Run: `.venv/bin/python -m pytest tests/test_review_state.py -v`

Expected: FAIL — `ModuleNotFoundError: No module named 'ontology.review'`

- [ ] **Step 3: 实现状态机**

Create `src/ontology/review/state.py`:

```python
"""双轨审核状态机。

纯逻辑：接收 ItemView，返回新状态。不 import 任何 ORM —— 让审核规则可以脱离
数据库被穷举测试，这是「一条不对不该拖垮整份提案」能成立的验证基础。
"""

from enum import StrEnum

from pydantic import BaseModel, Field


class Track(StrEnum):
    BIZ = "biz"
    MODEL = "model"


class Decision(StrEnum):
    ACCEPT = "accept"
    REJECT = "reject"


class ProposalStatus(StrEnum):
    DRAFT = "draft"
    UNDER_REVIEW = "under_review"
    APPROVED = "approved"
    PARTIAL = "partial"
    REJECTED = "rejected"
    APPLIED = "applied"


class DependencyNotMet(RuntimeError):
    """关系类变更的两端实体类型尚未确定。"""


class ItemView(BaseModel):
    """提案条目在审核逻辑中的视图。字段与 ProposalItem 对齐，但独立于 ORM。"""

    id: str
    kind: str
    target_name: str
    payload: dict = Field(default_factory=dict)
    evidence: list[dict] = Field(default_factory=list)
    biz_status: str = "pending"
    model_status: str = "pending"

    @property
    def is_accepted(self) -> bool:
        return self.biz_status == "accepted" and self.model_status == "accepted"

    @property
    def is_rejected(self) -> bool:
        return self.biz_status == "rejected" or self.model_status == "rejected"

    @property
    def is_pending(self) -> bool:
        return not self.is_accepted and not self.is_rejected


def apply_decision(item: ItemView, track: Track, decision: Decision) -> None:
    """把一个审核决策写回条目。就地修改。"""
    # 用 == 而不是 is：StrEnum 与裸字符串都能比较成功，
    # 写成 is 的话传 "biz" 会静默走到 else 分支，把业务轨判定写进建模轨。
    if track == Track.BIZ:
        item.biz_status = "accepted" if decision == Decision.ACCEPT else "rejected"
    else:
        item.model_status = "accepted" if decision == Decision.ACCEPT else "rejected"


def check_dependencies(
    item: ItemView, all_items: list[ItemView], baseline_entity_names: list[str]
) -> None:
    """校验条目的依赖是否满足，不满足则抛 DependencyNotMet。

    规则：关系类条目引用的两端实体类型，必须要么已存在于基线快照，要么在本提案中
    被两轨都接受。这是防止本体出现野指针的唯一关口。
    """
    if item.kind != "add_link_type":
        return

    from_entity = item.payload.get("from_entity")
    to_entity = item.payload.get("to_entity")

    accepted_entity_names = {
        other.payload.get("name")
        for other in all_items
        if other.kind == "add_entity_type" and other.is_accepted
    }

    for name in (from_entity, to_entity):
        if name in baseline_entity_names:
            continue
        if name in accepted_entity_names:
            continue
        raise DependencyNotMet(
            f"关系 {item.target_name} 依赖实体类型 {name}，"
            f"但它既不在基线版本中，也未在本提案中被两轨接受"
        )


def summarize_proposal(items: list[ItemView]) -> ProposalStatus:
    """汇总提案状态。任一条目未决 → 审核中；全部接受 → 通过；全部拒绝 → 驳回；其余 → 部分通过。"""
    if not items:
        return ProposalStatus.UNDER_REVIEW
    if any(item.is_pending for item in items):
        return ProposalStatus.UNDER_REVIEW
    if all(item.is_accepted for item in items):
        return ProposalStatus.APPROVED
    if all(item.is_rejected for item in items):
        return ProposalStatus.REJECTED
    return ProposalStatus.PARTIAL
```

Create `src/ontology/review/__init__.py`:

```python
from ontology.review.state import (
    Decision,
    DependencyNotMet,
    ItemView,
    ProposalStatus,
    Track,
    apply_decision,
    check_dependencies,
    summarize_proposal,
)

__all__ = [
    "Decision",
    "DependencyNotMet",
    "ItemView",
    "ProposalStatus",
    "Track",
    "apply_decision",
    "check_dependencies",
    "summarize_proposal",
]
```

- [ ] **Step 4: 跑测试确认通过**

Run: `.venv/bin/python -m pytest tests/test_review_state.py -v`

Expected: PASS（16 passed）

- [ ] **Step 5: Commit**

```bash
git add src/ontology/review tests/test_review_state.py
git commit -m "feat: add dual-track review state machine with dependency checks"
```

---

## Task 8: 双轨视图投影

设计文档 §6.2 的硬要求：**同一版本体，两个视图；客户视图中不得出现主键、外键、基数等建模术语**。这不是文案风格问题 —— 客户看到这些术语会失去判断能力，审核就退化成盲签。

本条的测试核心是一条**黑名单断言**：客户视图的序列化结果中不得出现建模术语。

**Files:**
- Create: `src/ontology/review/tracks.py`
- Modify: `src/ontology/review/__init__.py`
- Create: `tests/test_tracks.py`

- [ ] **Step 1: 写失败的测试**

Create `tests/test_tracks.py`:

```python
import json

import pytest

from ontology.review.state import ItemView
from ontology.review.tracks import biz_view, model_view

MODELING_TERMS = [
    "primary_key",
    "pk_columns",
    "cardinality",
    "one_to_many",
    "many_to_one",
    "foreign_key",
    "normal_form",
    # 内部枚举值也不该出现 —— 客户看到的是「新增对象」，不是 add_entity_type。
    "add_entity_type",
    "add_property",
    "add_link_type",
    "modify_entity_type",
    "remove_entity_type",
]


@pytest.fixture
def items() -> list[ItemView]:
    return [
        ItemView(
            id="i1",
            kind="add_entity_type",
            target_name="设备",
            payload={"name": "设备", "primary_key": ["dev_id"]},
            evidence=[{"table": "t_dev", "columns": ["dev_id", "sn"]}],
        ),
        ItemView(
            id="i2",
            kind="add_link_type",
            target_name="设备->工单",
            payload={
                "name": "设备-工单",
                "from_entity": "设备",
                "to_entity": "工单",
                "cardinality": "one_to_many",
            },
            evidence=[{"table": "t_dev", "columns": ["dev_id"]}],
        ),
    ]


def test_biz_view_uses_business_language(items):
    view = biz_view(items)
    assert view[0]["title"] == "新增对象：设备"
    assert view[1]["title"] == "新增关联：设备-工单"
    assert [entry["label"] for entry in view] == ["新增对象", "新增关联"]


def test_biz_view_never_exposes_modeling_terms(items):
    """客户视图不得出现任何建模术语 —— 出现了客户就没法审。"""
    dumped = json.dumps(biz_view(items), ensure_ascii=False)
    for term in MODELING_TERMS:
        assert term not in dumped, f"客户视图泄漏了建模术语: {term}"


def test_biz_view_shows_evidence_in_vendor_terms(items):
    """证据用客户认识的表名表述，不用内部字段名。"""
    dumped = json.dumps(biz_view(items), ensure_ascii=False)
    assert "t_dev" in dumped


def test_biz_view_carries_status(items):
    view = biz_view(items)
    assert all(entry["status"] == "pending" for entry in view)


def test_model_view_exposes_modeling_detail(items):
    view = model_view(items)
    dumped = json.dumps(view, ensure_ascii=False)
    assert "primary_key" in dumped
    assert "one_to_many" in dumped
    assert "dev_id" in dumped


def test_model_view_marks_link_dependency(items):
    """FDE 需要看到关系两端的依赖状态，否则无从判断为什么它还不能通过。"""
    view = model_view(items)
    link = next(entry for entry in view if entry["kind"] == "add_link_type")
    assert link["depends_on"] == ["设备", "工单"]


def test_model_view_entity_has_no_dependency(items):
    view = model_view(items)
    entity = next(entry for entry in view if entry["kind"] == "add_entity_type")
    assert entity["depends_on"] == []


def test_views_are_read_only_projections(items):
    """投影不得改动原条目 —— 否则审核界面刷新会污染状态机。"""
    before = [item.model_dump() for item in items]
    biz_view(items)
    model_view(items)
    assert [item.model_dump() for item in items] == before
```

- [ ] **Step 2: 跑测试确认失败**

Run: `.venv/bin/python -m pytest tests/test_tracks.py -v`

Expected: FAIL — `ModuleNotFoundError: No module named 'ontology.review.tracks'`

- [ ] **Step 3: 实现视图投影**

Create `src/ontology/review/tracks.py`:

```python
"""双轨视图投影。

同一份提案，两个视图。这里只做投影，不做判定 —— 投影函数必须是纯函数，
否则审核界面刷新会反向污染状态机。
"""

from ontology.review.state import ItemView

# 客户视图的输出里绝不允许出现的键。测试会把它当黑名单断言。
MODELING_TERMS = frozenset(
    {
        "primary_key",
        "pk_columns",
        "cardinality",
        "one_to_one",
        "one_to_many",
        "many_to_one",
        "many_to_many",
        "foreign_key",
        "normal_form",
        # 内部条目标签同样是建模语汇。客户只需要「新增关联」这个说法。
        "add_entity_type",
        "add_property",
        "add_link_type",
        "modify_entity_type",
        "remove_entity_type",
    }
)

KIND_LABELS = {
    "add_entity_type": "新增对象",
    "add_property": "新增字段",
    "add_link_type": "新增关联",
    "modify_entity_type": "修改对象",
    "remove_entity_type": "删除对象",
}


def _source_tables(item: ItemView) -> list[str]:
    """从证据里抽出客户认识的来源表名，去重且保持出现顺序。"""
    tables: list[str] = []
    for record in item.evidence:
        table = record.get("table")
        if table and table not in tables:
            tables.append(table)
    return tables


def _truncate(value: str, limit: int) -> str:
    """只在明显过长时才截断，避免把普通值截成半个词。"""
    return value if len(value) <= limit else value[: limit - 1] + "…"


def biz_view(items: list[ItemView]) -> list[dict]:
    """客户视图。只用业务语言，不含任何建模术语。"""
    view: list[dict] = []
    for item in items:
        # fallback 必须是中文描述：未知 kind 时把内部枚举值泄给客户，
        # 比显示一个笼统的说法更糟。
        label = KIND_LABELS.get(item.kind, "其他变更")
        entry: dict = {
            "id": item.id,
            "label": label,
            "title": f"{label}：{item.target_name}",
            "status": item.biz_status,
        }
        tables = _source_tables(item)
        if tables:
            entry["detail"] = _truncate(
                "根据这些数据推断：" + "、".join(tables), 300
            )
        view.append(entry)
    return view


def model_view(items: list[ItemView]) -> list[dict]:
    """FDE 视图。暴露建模细节与依赖关系。"""
    view: list[dict] = []
    for item in items:
        entry: dict = {
            "id": item.id,
            "kind": item.kind,
            "target_name": item.target_name,
            "payload": dict(item.payload),
            "biz_status": item.biz_status,
            "model_status": item.model_status,
            "depends_on": _dependencies(item),
            "evidence": list(item.evidence),
        }
        view.append(entry)
    return view


def _dependencies(item: ItemView) -> list[str]:
    """关系类的两端的依赖。其他类型无依赖。"""
    if item.kind != "add_link_type":
        return []
    return [name for name in (item.payload.get("from_entity"), item.payload.get("to_entity")) if name]
```

- [ ] **Step 4: 导出**

Modify `src/ontology/review/__init__.py` — 改成：

```python
from ontology.review.state import (
    Decision,
    DependencyNotMet,
    ItemView,
    ProposalStatus,
    Track,
    apply_decision,
    check_dependencies,
    summarize_proposal,
)
from ontology.review.tracks import biz_view, model_view

__all__ = [
    "Decision",
    "DependencyNotMet",
    "ItemView",
    "ProposalStatus",
    "Track",
    "apply_decision",
    "biz_view",
    "check_dependencies",
    "model_view",
    "summarize_proposal",
]
```

- [ ] **Step 5: 跑测试确认通过**

Run: `.venv/bin/python -m pytest tests/test_tracks.py -v`

Expected: PASS（8 passed）

- [ ] **Step 6: Commit**

```bash
git add src/ontology/review tests/test_tracks.py
git commit -m "feat: add dual-track view projections"
```

---

## Task 9: 快照 diff 引擎

设计文档 §6.3：下一轮抽取**不是重新生成，而是基于上一版快照产 diff**。这是迭代能收敛而不振荡的机制保证。本条的产物同时是 §6.4 收敛判定（"连续两轮 diff 小于阈值"）的输入。

**Files:**
- Create: `src/ontology/diff.py`
- Create: `tests/test_diff.py`

- [ ] **Step 1: 写失败的测试**

Create `tests/test_diff.py`:

```python
from ontology.diff import diff_snapshots
from ontology.snapshot import EntityTypeSnapshot, LinkTypeSnapshot, OntologySnapshot, PropertySnapshot


def _empty() -> OntologySnapshot:
    return OntologySnapshot()


def _with_device(description: str = "生产设备") -> OntologySnapshot:
    return OntologySnapshot(
        entity_types=[
            EntityTypeSnapshot(
                name="设备",
                description=description,
                primary_key=["dev_id"],
                properties=[PropertySnapshot(name="sn", data_type="text")],
            )
        ]
    )


def test_identical_snapshots_produce_empty_diff():
    snapshot = _with_device()
    diff = diff_snapshots(snapshot, snapshot.model_copy(deep=True))
    assert diff.is_empty
    assert diff.size() == 0


def test_added_entity_type_is_detected():
    diff = diff_snapshots(_empty(), _with_device())
    assert [change.name for change in diff.added_entity_types] == ["设备"]
    assert diff.removed_entity_types == []
    assert diff.modified_entity_types == []


def test_removed_entity_type_is_detected():
    diff = diff_snapshots(_with_device(), _empty())
    assert [change.name for change in diff.removed_entity_types] == ["设备"]


def test_modified_description_is_detected():
    diff = diff_snapshots(_with_device("生产设备"), _with_device("生产设备（含备用机）"))
    assert [change.name for change in diff.modified_entity_types] == ["设备"]
    assert diff.added_entity_types == []
    assert diff.removed_entity_types == []


def test_added_property_is_reported_inside_modified_entity():
    before = _with_device()
    after = _with_device()
    after.entity_types[0].properties.append(PropertySnapshot(name="model", data_type="text"))

    diff = diff_snapshots(before, after)
    assert [change.name for change in diff.modified_entity_types] == ["设备"]
    assert diff.modified_entity_types[0].changed_properties == ["model"]


def test_property_order_does_not_count_as_change():
    """属性顺序无关语义。若顺序也算变更，LLM 每轮换个顺序就会让收敛判定失效。"""
    before = EntityTypeSnapshot(
        name="设备",
        properties=[PropertySnapshot(name="sn"), PropertySnapshot(name="model")],
    )
    after = EntityTypeSnapshot(
        name="设备",
        properties=[PropertySnapshot(name="model"), PropertySnapshot(name="sn")],
    )
    diff = diff_snapshots(
        OntologySnapshot(entity_types=[before]), OntologySnapshot(entity_types=[after])
    )
    assert diff.is_empty


def test_added_link_type_is_detected():
    after = _with_device()
    after.entity_types.append(EntityTypeSnapshot(name="工单"))
    after.link_types.append(
        LinkTypeSnapshot(
            name="设备-工单", from_entity="设备", to_entity="工单", cardinality="one_to_many"
        )
    )
    diff = diff_snapshots(_with_device(), after)
    assert [change.name for change in diff.added_link_types] == ["设备-工单"]


def test_changed_cardinality_marks_link_as_modified():
    before = _with_device()
    before.entity_types.append(EntityTypeSnapshot(name="工单"))
    before.link_types.append(
        LinkTypeSnapshot(
            name="设备-工单", from_entity="设备", to_entity="工单", cardinality="one_to_many"
        )
    )
    after = before.model_copy(deep=True)
    after.link_types[0].cardinality = "many_to_many"

    diff = diff_snapshots(before, after)
    assert [change.name for change in diff.modified_link_types] == ["设备-工单"]


def test_diff_size_counts_all_changes():
    after = _with_device()
    after.entity_types.append(EntityTypeSnapshot(name="工单"))
    diff = diff_snapshots(_empty(), after)
    assert diff.size() == 2


def test_entity_type_lookup_by_name():
    snapshot = _with_device()
    assert snapshot.entity_type("设备").description == "生产设备"
    assert snapshot.entity_type("不存在") is None
```

- [ ] **Step 2: 跑测试确认失败**

Run: `.venv/bin/python -m pytest tests/test_diff.py -v`

Expected: FAIL — `ModuleNotFoundError: No module named 'ontology.diff'`

- [ ] **Step 3: 实现 diff**

Create `src/ontology/diff.py`:

```python
"""快照 diff。

设计文档 §6.3：下一轮抽取基于上一版快照产 diff，而不是重新生成 —— 这是闭环能
收敛而不是振荡的机制保证。§6.4 的收敛判定（连续两轮 diff 小于阈值）直接消费本模块。
"""

from pydantic import BaseModel, Field

from ontology.snapshot import EntityTypeSnapshot, LinkTypeSnapshot, OntologySnapshot


class EntityTypeChange(BaseModel):
    kind: str  # added | removed | modified
    name: str
    before: EntityTypeSnapshot | None = None
    after: EntityTypeSnapshot | None = None
    changed_properties: list[str] = Field(default_factory=list)


class LinkTypeChange(BaseModel):
    kind: str  # added | removed | modified
    name: str
    before: LinkTypeSnapshot | None = None
    after: LinkTypeSnapshot | None = None


class OntologyDiff(BaseModel):
    added_entity_types: list[EntityTypeChange] = Field(default_factory=list)
    removed_entity_types: list[EntityTypeChange] = Field(default_factory=list)
    modified_entity_types: list[EntityTypeChange] = Field(default_factory=list)
    added_link_types: list[LinkTypeChange] = Field(default_factory=list)
    removed_link_types: list[LinkTypeChange] = Field(default_factory=list)
    modified_link_types: list[LinkTypeChange] = Field(default_factory=list)

    @property
    def is_empty(self) -> bool:
        return self.size() == 0

    def size(self) -> int:
        return (
            len(self.added_entity_types)
            + len(self.removed_entity_types)
            + len(self.modified_entity_types)
            + len(self.added_link_types)
            + len(self.removed_link_types)
            + len(self.modified_link_types)
        )


def _normalized_entity(entity: EntityTypeSnapshot) -> dict:
    """属性按名排序后再比较 —— 顺序无关语义，但若顺序算变更，
    LLM 每轮换个输出顺序就会让收敛判定永远达不成。"""
    return {
        "name": entity.name,
        "description": entity.description,
        "primary_key": sorted(entity.primary_key),
        "properties": sorted(
            (
                {
                    "name": prop.name,
                    "data_type": prop.data_type,
                    "nullable": prop.nullable,
                }
                for prop in entity.properties
            ),
            key=lambda p: p["name"],
        ),
    }


def _normalized_link(link: LinkTypeSnapshot) -> dict:
    return {
        "name": link.name,
        "from_entity": link.from_entity,
        "to_entity": link.to_entity,
        "cardinality": link.cardinality,
    }


def _changed_property_names(
    before: EntityTypeSnapshot, after: EntityTypeSnapshot
) -> list[str]:
    before_props = {prop.name: prop for prop in before.properties}
    after_props = {prop.name: prop for prop in after.properties}
    changed = []
    for name in sorted(set(before_props) | set(after_props)):
        left = before_props.get(name)
        right = after_props.get(name)
        if left is None or right is None:
            changed.append(name)
        elif (left.data_type, left.nullable) != (right.data_type, right.nullable):
            changed.append(name)
    return changed


def diff_snapshots(before: OntologySnapshot, after: OntologySnapshot) -> OntologyDiff:
    """比较两个快照。证据（evidence）不参与比较 —— 证据是来源，不是本体的一部分。"""
    diff = OntologyDiff()

    before_entities = {et.name: et for et in before.entity_types}
    after_entities = {et.name: et for et in after.entity_types}

    for name in sorted(set(after_entities) - set(before_entities)):
        diff.added_entity_types.append(
            EntityTypeChange(kind="added", name=name, after=after_entities[name])
        )
    for name in sorted(set(before_entities) - set(after_entities)):
        diff.removed_entity_types.append(
            EntityTypeChange(kind="removed", name=name, before=before_entities[name])
        )
    for name in sorted(set(before_entities) & set(after_entities)):
        if _normalized_entity(before_entities[name]) != _normalized_entity(after_entities[name]):
            diff.modified_entity_types.append(
                EntityTypeChange(
                    kind="modified",
                    name=name,
                    before=before_entities[name],
                    after=after_entities[name],
                    changed_properties=_changed_property_names(
                        before_entities[name], after_entities[name]
                    ),
                )
            )

    before_links = {link.name: link for link in before.link_types}
    after_links = {link.name: link for link in after.link_types}

    for name in sorted(set(after_links) - set(before_links)):
        diff.added_link_types.append(
            LinkTypeChange(kind="added", name=name, after=after_links[name])
        )
    for name in sorted(set(before_links) - set(after_links)):
        diff.removed_link_types.append(
            LinkTypeChange(kind="removed", name=name, before=before_links[name])
        )
    for name in sorted(set(before_links) & set(after_links)):
        if _normalized_link(before_links[name]) != _normalized_link(after_links[name]):
            diff.modified_link_types.append(
                LinkTypeChange(
                    kind="modified",
                    name=name,
                    before=before_links[name],
                    after=after_links[name],
                )
            )

    return diff
```

- [ ] **Step 4: 跑测试确认通过**

Run: `.venv/bin/python -m pytest tests/test_diff.py -v`

Expected: PASS（10 passed）

- [ ] **Step 5: Commit**

```bash
git add src/ontology/diff.py tests/test_diff.py
git commit -m "feat: add snapshot diff engine with t064-insensitive comparison"
```

---

## Task 10: 提案应用与版本冻结

闭环的最后一环：把审核通过的条目落成本体的新版本，并支持冻结。设计文档 §2.2 的机制在这里闭环 —— agent 的产出只有经过这一条路径才能进入本体。

**Files:**
- Create: `src/ontology/review/apply.py`
- Modify: `src/ontology/review/__init__.py`
- Create: `tests/test_apply.py`

- [ ] **Step 1: 写失败的测试**

Create `tests/test_apply.py`:

```python
import pytest

from ontology.review.apply import apply_items, freeze_after_apply
from ontology.review.state import ItemView
from ontology.snapshot import EntityTypeSnapshot, OntologySnapshot
from ontology.store import InMemoryOntologyStore, VersionFrozenError


@pytest.fixture
def store() -> InMemoryOntologyStore:
    return InMemoryOntologyStore()


@pytest.fixture
def project_id(store) -> str:
    return store.create_project("acme")


def _entity_item(name: str, pk: list[str] | None = None, accepted: bool = True) -> ItemView:
    status = "accepted" if accepted else "pending"
    return ItemView(
        id=f"item-{name}",
        kind="add_entity_type",
        target_name=name,
        payload={"name": name, "primary_key": pk or [f"{name}_id"], "description": None},
        biz_status=status,
        model_status=status,
    )


def test_apply_creates_new_version_with_base_content(store, project_id):
    base = store.create_version(project_id)
    store.save_snapshot(
        base.id, OntologySnapshot(entity_types=[EntityTypeSnapshot(name="设备")])
    )

    new_id = apply_items(store, project_id, base.id, [_entity_item("工单")])

    new_version = store.get_version(new_id)
    assert new_version.seq == 2
    assert new_version.parent_version_id == base.id
    assert new_version.status == "draft"
    names = [et.name for et in store.get_snapshot(new_id).entity_types]
    assert names == ["设备", "工单"]


def test_base_version_is_untouched(store, project_id):
    """应用提案绝不能就地改基线 —— 否则客户没法回退到"他们签字过的那一版"。"""
    base = store.create_version(project_id)
    store.save_snapshot(
        base.id, OntologySnapshot(entity_types=[EntityTypeSnapshot(name="设备")])
    )

    apply_items(store, project_id, base.id, [_entity_item("工单")])

    assert [et.name for et in store.get_snapshot(base.id).entity_types] == ["设备"]


def test_pending_items_are_not_applied(store, project_id):
    base = store.create_version(project_id)
    new_id = apply_items(store, project_id, base.id, [_entity_item("工单", accepted=False)])
    assert store.get_snapshot(new_id).entity_types == []


def test_rejected_items_are_not_applied(store, project_id):
    base = store.create_version(project_id)
    rejected = _entity_item("工单")
    rejected.model_status = "rejected"
    new_id = apply_items(store, project_id, base.id, [rejected])
    assert store.get_snapshot(new_id).entity_types == []


def test_apply_without_base_version_works_for_first_version(store, project_id):
    """第一轮抽取没有基线版本，base_version_id 为 None。"""
    new_id = apply_items(store, project_id, None, [_entity_item("设备")])
    version = store.get_version(new_id)
    assert version.seq == 1
    assert version.parent_version_id is None
    assert [et.name for et in store.get_snapshot(new_id).entity_types] == ["设备"]


def test_apply_is_idempotent_for_same_items(store, project_id):
    """重复 apply 同一批条目应产出内容相同的新版本，而不是重复叠加。"""
    base = store.create_version(project_id)
    items = [_entity_item("设备")]
    first = apply_items(store, project_id, base.id, items)
    second = apply_items(store, project_id, base.id, items)

    first_names = [et.name for et in store.get_snapshot(first).entity_types]
    second_names = [et.name for et in store.get_snapshot(second).entity_types]
    assert first_names == second_names == ["设备"]


def test_add_property_item_is_applied(store, project_id):
    base = store.create_version(project_id)
    store.save_snapshot(
        base.id, OntologySnapshot(entity_types=[EntityTypeSnapshot(name="设备")])
    )
    prop = ItemView(
        id="p1",
        kind="add_property",
        target_name="设备.sn",
        payload={"entity": "设备", "name": "sn", "data_type": "text", "nullable": False},
        biz_status="accepted",
        model_status="accepted",
    )

    new_id = apply_items(store, project_id, base.id, [prop])

    device = store.get_snapshot(new_id).entity_type("设备")
    assert [p.name for p in device.properties] == ["sn"]
    assert device.properties[0].nullable is False


def test_add_property_to_missing_entity_raises(store, project_id):
    base = store.create_version(project_id)
    prop = ItemView(
        id="p1",
        kind="add_property",
        target_name="不存在.sn",
        payload={"entity": "不存在", "name": "sn", "data_type": "text"},
        biz_status="accepted",
        model_status="accepted",
    )
    with pytest.raises(ValueError, match="不存在"):
        apply_items(store, project_id, base.id, [prop])


def test_add_link_type_item_is_applied(store, project_id):
    base = store.create_version(project_id)
    store.save_snapshot(
        base.id,
        OntologySnapshot(
            entity_types=[EntityTypeSnapshot(name="设备"), EntityTypeSnapshot(name="工单")]
        ),
    )
    link = ItemView(
        id="l1",
        kind="add_link_type",
        target_name="设备->工单",
        payload={
            "name": "设备-工单",
            "from_entity": "设备",
            "to_entity": "工单",
            "cardinality": "one_to_many",
        },
        biz_status="accepted",
        model_status="accepted",
    )

    new_id = apply_items(store, project_id, base.id, [link])

    links = store.get_snapshot(new_id).link_types
    assert [link.name for link in links] == ["设备-工单"]


def test_remove_entity_type_drops_it_and_its_links(store, project_id):
    """删掉实体类型必须连带删掉引用它的关系，否则本体里会留下野指针。"""
    base = store.create_version(project_id)
    from ontology.snapshot import LinkTypeSnapshot

    store.save_snapshot(
        base.id,
        OntologySnapshot(
            entity_types=[EntityTypeSnapshot(name="设备"), EntityTypeSnapshot(name="工单")],
            link_types=[
                LinkTypeSnapshot(
                    name="设备-工单",
                    from_entity="设备",
                    to_entity="工单",
                    cardinality="one_to_many",
                )
            ],
        ),
    )
    removal = ItemView(
        id="r1",
        kind="remove_entity_type",
        target_name="工单",
        payload={"name": "工单"},
        biz_status="accepted",
        model_status="accepted",
    )

    new_id = apply_items(store, project_id, base.id, [removal])

    snapshot = store.get_snapshot(new_id)
    assert [et.name for et in snapshot.entity_types] == ["设备"]
    assert snapshot.link_types == []


def test_modify_entity_type_updates_description(store, project_id):
    base = store.create_version(project_id)
    store.save_snapshot(
        base.id, OntologySnapshot(entity_types=[EntityTypeSnapshot(name="设备", description="旧")])
    )
    modification = ItemView(
        id="m1",
        kind="modify_entity_type",
        target_name="设备",
        payload={"name": "设备", "description": "生产设备（含备用机）"},
        biz_status="accepted",
        model_status="accepted",
    )

    new_id = apply_items(store, project_id, base.id, [modification])

    assert store.get_snapshot(new_id).entity_type("设备").description == "生产设备（含备用机）"


def test_freeze_after_apply_blocks_further_writes(store, project_id):
    base = store.create_version(project_id)
    new_id = apply_items(store, project_id, base.id, [_entity_item("设备")])
    freeze_after_apply(store, new_id)

    assert store.get_version(new_id).status == "frozen"
    with pytest.raises(VersionFrozenError):
        store.save_snapshot(new_id, OntologySnapshot())


def test_chained_apply_keeps_accumulating(store, project_id):
    """这就是 §6.3 的累积语义：第二轮基于第一轮的结果，而不是从零重来。"""
    v1 = apply_items(store, project_id, None, [_entity_item("设备")])
    v2 = apply_items(store, project_id, v1, [_entity_item("工单")])
    v3 = apply_items(store, project_id, v2, [_entity_item("备件")])

    assert [et.name for et in store.get_snapshot(v3).entity_types] == ["设备", "工单", "备件"]
    assert store.get_version(v3).seq == 3
    assert store.get_version(v3).parent_version_id == v2
```

- [ ] **Step 2: 跑测试确认失败**

Run: `.venv/bin/python -m pytest tests/test_apply.py -v`

Expected: FAIL — `ModuleNotFoundError: No module named 'ontology.review.apply'`

- [ ] **Step 3: 实现应用逻辑**

Create `src/ontology/review/apply.py`:

```python
"""把审核通过的提案条目落成新的本体版本。

设计文档 §2.2 的机制在这里闭环：agent 的产出只有经过这条路径才能进入本体。
基线版本永远不被就地修改 —— 客户签字过的版本必须能原样回看。
"""

from ontology.review.state import ItemView
from ontology.snapshot import (
    EntityTypeSnapshot,
    LinkTypeSnapshot,
    OntologySnapshot,
    PropertySnapshot,
)
from ontology.store.base import OntologyStore


def apply_items(
    store: OntologyStore,
    project_id: str,
    base_version_id: str | None,
    items: list[ItemView],
) -> str:
    """把两轨都接受的条目应用到基线版本，产生新版本，返回新版本 id。

    base_version_id 为 None 表示第一轮抽取（从零构建）。
    """
    base = (
        store.get_snapshot(base_version_id)
        if base_version_id
        else OntologySnapshot()
    )
    snapshot = base.model_copy(deep=True)

    for item in items:
        if not item.is_accepted:
            continue
        _apply_item(snapshot, item)

    version = store.create_version(project_id, parent_version_id=base_version_id)
    store.save_snapshot(version.id, snapshot)
    return version.id


def freeze_after_apply(store: OntologyStore, version_id: str) -> None:
    """冻结版本。后续抽取只能在此之上追加，不可覆盖（§6.3）。"""
    store.freeze_version(version_id)


def _apply_item(snapshot: OntologySnapshot, item: ItemView) -> None:
    if item.kind == "add_entity_type":
        _add_entity_type(snapshot, item)
    elif item.kind == "modify_entity_type":
        _modify_entity_type(snapshot, item)
    elif item.kind == "remove_entity_type":
        _remove_entity_type(snapshot, item)
    elif item.kind == "add_property":
        _add_property(snapshot, item)
    elif item.kind == "add_link_type":
        _add_link_type(snapshot, item)
    else:
        raise ValueError(f"未知的条目类型: {item.kind}")


def _add_entity_type(snapshot: OntologySnapshot, item: ItemView) -> None:
    name = item.payload.get("name") or item.target_name
    if snapshot.entity_type(name) is not None:
        # 幂等：重复应用同一提案不应叠加出重复类型。
        return
    snapshot.entity_types.append(
        EntityTypeSnapshot(
            name=name,
            description=item.payload.get("description"),
            primary_key=list(item.payload.get("primary_key") or []),
            evidence=list(item.evidence),
        )
    )


def _modify_entity_type(snapshot: OntologySnapshot, item: ItemView) -> None:
    name = item.payload.get("name") or item.target_name
    entity = snapshot.entity_type(name)
    if entity is None:
        raise ValueError(f"待修改的实体类型不存在: {name}")
    if "description" in item.payload:
        entity.description = item.payload["description"]
    if item.payload.get("primary_key"):
        entity.primary_key = list(item.payload["primary_key"])


def _remove_entity_type(snapshot: OntologySnapshot, item: ItemView) -> None:
    name = item.payload.get("name") or item.target_name
    snapshot.entity_types = [et for et in snapshot.entity_types if et.name != name]
    # 连带清掉引用它的关系，否则本体里会留下指向不存在实体的野指针。
    snapshot.link_types = [
        link
        for link in snapshot.link_types
        if link.from_entity != name and link.to_entity != name
    ]


def _add_property(snapshot: OntologySnapshot, item: ItemView) -> None:
    entity_name = item.payload.get("entity")
    entity = snapshot.entity_type(entity_name)
    if entity is None:
        raise ValueError(f"实体类型不存在: {entity_name}")

    prop_name = item.payload.get("name")
    if any(prop.name == prop_name for prop in entity.properties):
        return
    entity.properties.append(
        PropertySnapshot(
            name=prop_name,
            data_type=item.payload.get("data_type", "text"),
            nullable=item.payload.get("nullable", True),
            source_ref=dict(item.payload.get("source_ref") or {}),
            evidence=list(item.evidence),
        )
    )


def _add_link_type(snapshot: OntologySnapshot, item: ItemView) -> None:
    from_entity = item.payload.get("from_entity")
    to_entity = item.payload.get("to_entity")
    for name in (from_entity, to_entity):
        if snapshot.entity_type(name) is None:
            raise ValueError(f"关系 {item.target_name} 引用的实体类型不存在: {name}")

    link_name = item.payload.get("name") or item.target_name
    if any(link.name == link_name for link in snapshot.link_types):
        return
    snapshot.link_types.append(
        LinkTypeSnapshot(
            name=link_name,
            from_entity=from_entity,
            to_entity=to_entity,
            cardinality=item.payload.get("cardinality", "many_to_many"),
            evidence=list(item.evidence),
        )
    )
```

- [ ] **Step 4: 导出**

Modify `src/ontology/review/__init__.py` — 在 `from ontology.review.tracks import ...` 之前插入一行，改成：

```python
from ontology.review.apply import apply_items, freeze_after_apply
from ontology.review.state import (
    Decision,
    DependencyNotMet,
    ItemView,
    ProposalStatus,
    Track,
    apply_decision,
    check_dependencies,
    summarize_proposal,
)
from ontology.review.tracks import biz_view, model_view

__all__ = [
    "Decision",
    "DependencyNotMet",
    "ItemView",
    "ProposalStatus",
    "Track",
    "apply_decision",
    "apply_items",
    "biz_view",
    "check_dependencies",
    "freeze_after_apply",
    "model_view",
    "summarize_proposal",
]
```

- [ ] **Step 5: 跑测试确认通过**

Run: `.venv/bin/python -m pytest tests/test_apply.py -v`

Expected: PASS（13 passed）

- [ ] **Step 6: 跑全量测试**

Run: `.venv/bin/python -m pytest -v`

Expected: PASS（全部通过，含 PG 契约测试）

- [ ] **Step 7: Commit**

```bash
git add src/ontology/review tests/test_apply.py
git commit -m "feat: apply accepted proposal items into new ontology versions"
```

---

## 完成标准

本计划完成的判定条件，全部可验证：

- [ ] `.venv/bin/python -m pytest` 全绿（含 PostgreSQL 契约测试的两套实现）
- [ ] `ontology-store` 抽象有两个实现，跑同一套契约测试
- [ ] 领域层的纯函数模块不直接依赖 ORM：运行
      `grep -n 'ontology\.models' src/ontology/diff.py src/ontology/review/state.py src/ontology/review/tracks.py`
      预期无输出。`review/apply.py` 不在此列 —— 它本来就要调 `OntologyStore`
- [ ] 客户视图的序列化结果不含建模术语（`test_biz_view_never_exposes_modeling_terms` 守住）
- [ ] 已冻结版本拒绝任何写入（`test_frozen_version_rejects_writes` 守住）
- [ ] **ActionType / Function 的模型与快照占位已落地**（Task 3.5），且 `save_snapshot` / `get_snapshot` 对这两个列表无损往返
- [ ] 切片 1 的快照里 `action_types` / `functions` 恒为空列表，diff 引擎对它们报「无差异」（Task 3.5 Step 4）
- [ ] `EntityType` 已带 `parent` / `aliases` / `disjoint_with`，且层级纯函数能检出环与悬空父引用（Task 3.6）
- [ ] diff 引擎对 `parent` 变更产出 `kind="hierarchy"` 且带 `impacted` 后代列表（Task 3.6 Step 6）
- [ ] `WriteBindingSnapshot` 已进快照契约（Task 3.5 Step 4），含 `value_map` / `supported_values` / `conflict_field` / `key_expr` 四个字段，且切片 1 恒为空
- [ ] 切片 1 **不存在任何可执行的写回路径** —— 这是只读闭环的底线，须有测试守住（`test_no_write_path_in_slice1`）
- [ ] 关系类条目在两端实体未接受时被拦截（`test_link_acceptance_requires_both_ends_accepted` 守住）
- [ ] 链式 apply 累积而不重置（`test_chained_apply_keeps_accumulating` 守住）

## 交接给下一份计划

下一份（数据接入与抽取）依赖本计划产出的这些接口：

- `OntologyStore` 协议：`create_project` / `create_version` / `get_snapshot` / `save_snapshot` / `freeze_version` / `latest_version`
- `OntologySnapshot` / `EntityTypeSnapshot` / `PropertySnapshot` / `LinkTypeSnapshot` / `ActionTypeSnapshot` / `FunctionSnapshot`
- `OntologyDiff` 与 `diff_snapshots` —— 抽取引擎用它算两轮之间的推进量
- `ProposalItem`（ORM）：抽取引擎把候选本体拆成 `add_entity_type` / `add_property` / `add_link_type` 三类条目写入
- `WriteBindingSnapshot`：切片 2 的写回绑定。**切片 1 恒为空，切片 1 的抽取引擎不得产出任何 `add_write_binding` 条目**
- `review.state.ItemView`：从 `ProposalItem` 转换而来，是审核逻辑的统一输入

约定：抽取引擎**只写 ProposalItem，不碰 ontology_version 表**。本体版本的每一次推进都必须经过 `apply_items`。

> **关于 ActionType / Function**：`ActionTypeSnapshot` / `FunctionSnapshot` 与对应表已就位，但**抽取引擎在切片 1 不产出 `add_action_type` / `add_function` 条目**。切片 2 才启用。
> 抽取范围仍由**决策清单**限定（设计规格 §2.6 / §7.2）—— 决策清单没提到的数据源字段，本轮不抽。
