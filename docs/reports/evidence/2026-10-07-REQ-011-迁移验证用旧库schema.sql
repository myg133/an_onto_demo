
CREATE TABLE IF NOT EXISTS projects (
  id          UUID PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ontology_versions (
  id            UUID PRIMARY KEY,
  project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  seq           INT  NOT NULL,
  frozen_at     TIMESTAMPTZ,                     -- ← NULL = 未冻结（三态之一）
  -- 抽取来源。**NULL = 人工修订**（ADR-001：apply 时原样取提案的值，
  -- 缺失记 NULL，不继承 base、不推断）
  extracted_by  JSONB,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, seq)
);

CREATE TABLE IF NOT EXISTS object_types (
  id             UUID PRIMARY KEY,
  version_id     UUID NOT NULL REFERENCES ontology_versions(id) ON DELETE CASCADE,
  key            TEXT NOT NULL,
  display_name   TEXT NOT NULL,
  description    TEXT,                            -- ← NULL 有意义（AC-010）
  aliases        JSONB NOT NULL DEFAULT '[]',    -- 占位（AC-003）
  parent_key     TEXT,                            -- 存父的 key 而非 id（Task 4）
  disjoint_with  JSONB NOT NULL DEFAULT '[]',     -- 占位（AC-003）
  properties     JSONB NOT NULL DEFAULT '[]',
  evidence       JSONB NOT NULL DEFAULT '[]',
  ordinal        INT  NOT NULL DEFAULT 0,
  UNIQUE (version_id, key)
);

CREATE TABLE IF NOT EXISTS link_types (
  id           UUID PRIMARY KEY,
  version_id   UUID NOT NULL REFERENCES ontology_versions(id) ON DELETE CASCADE,
  key          TEXT NOT NULL,
  from_type    TEXT NOT NULL,
  to_type      TEXT NOT NULL,
  cardinality  TEXT NOT NULL,
  semantic     TEXT NOT NULL,
  evidence     JSONB NOT NULL DEFAULT '[]',
  ordinal      INT  NOT NULL DEFAULT 0,
  UNIQUE (version_id, key)
);

CREATE TABLE IF NOT EXISTS action_types (
  id              UUID PRIMARY KEY,
  version_id      UUID NOT NULL REFERENCES ontology_versions(id) ON DELETE CASCADE,
  key             TEXT NOT NULL,
  target          TEXT NOT NULL,
  parameters      JSONB NOT NULL DEFAULT '[]',
  preconditions   JSONB NOT NULL DEFAULT '[]',
  effects         JSONB NOT NULL DEFAULT '[]',   -- WriteBinding 占位（AC-003）
  permission      JSONB NOT NULL DEFAULT '{}',
  audit_policy    JSONB NOT NULL DEFAULT '{}',
  evidence        JSONB NOT NULL DEFAULT '[]',
  UNIQUE (version_id, key)
);

CREATE TABLE IF NOT EXISTS functions (
  id             UUID PRIMARY KEY,
  version_id     UUID NOT NULL REFERENCES ontology_versions(id) ON DELETE CASCADE,
  key            TEXT NOT NULL,
  signature      TEXT NOT NULL,
  allowed_types  JSONB NOT NULL DEFAULT '[]',
  evidence       JSONB NOT NULL DEFAULT '[]',
  ordinal        INT NOT NULL DEFAULT 0,
  UNIQUE (version_id, key)
);

CREATE TABLE IF NOT EXISTS proposals (
  id               UUID PRIMARY KEY,
  project_id       UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  base_version_id  UUID NOT NULL REFERENCES ontology_versions(id),
  round            INT  NOT NULL,
  extracted_by     JSONB,                          -- ← NULL = 人工修订（ADR-001）
  status           TEXT NOT NULL DEFAULT 'pending',
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS proposal_items (
  id           UUID PRIMARY KEY,
  proposal_id  UUID NOT NULL REFERENCES proposals(id) ON DELETE CASCADE,
  seq          INT  NOT NULL,
  kind         TEXT NOT NULL,
  target_key   TEXT NOT NULL,
  payload      JSONB NOT NULL,
  biz_status   TEXT NOT NULL DEFAULT 'pending',
  model_status TEXT NOT NULL DEFAULT 'pending',
  UNIQUE (proposal_id, seq)
);

-- REQ-010 · 业务概念组（§7.1 阶段 3）
--
-- 【红线】这张表**存的是展示层声明**，不参与任何落库结构：
-- 它没有外键指向 object_types（成员存 JSONB 数组），所以
-- 「去掉分组声明」不需要动 object_types 一行。
-- 悬空成员由 `domain::concept::validate_groups` 在 apply 期拦（见该文件理由①）。
CREATE TABLE IF NOT EXISTS concept_groups (
  id            UUID PRIMARY KEY,
  version_id    UUID NOT NULL REFERENCES ontology_versions(id) ON DELETE CASCADE,
  key           TEXT NOT NULL,
  display_name  TEXT NOT NULL,
  members       JSONB NOT NULL DEFAULT '[]',
  scope_hint    TEXT,                           -- ← NULL = 还没写说明（三态之一）
  extracted_by  JSONB,                          -- ← NULL = 人工修订（ADR-001）
  evidence      JSONB NOT NULL DEFAULT '[]',
  ordinal       INT NOT NULL DEFAULT 0,
  UNIQUE (version_id, key)
);


ALTER TABLE ontology_versions ADD COLUMN IF NOT EXISTS extracted_by JSONB;

