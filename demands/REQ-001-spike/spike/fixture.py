"""内置规范例子库 —— 用设计规格 §2.3 举的那个经典例子

这组表刻意复刻了规格里反复引用的三张表：
  t_dev / t_device_info / dev_backup

规格 §2.3 的原问题是：「这三张表字段重叠但不完全相同，
究竟是 3 个类型 + Link，还是该合并成 1 个类型？」

**这两者所依据的数据长得一模一样** —— 决定它的是业务语义，不是数据形态。
这正是「抽取负责发现，人负责定义」那个主张的落脚点。

用它做自测的好处：判读结果可以对照规格里已写明的答案，不需要外部依赖。
"""

from __future__ import annotations

from core import Column, Schema, Table

_TABLES = (
    Table(
        name="t_dev",
        comment="设备台账",
        columns=(
            Column("dev_id", "integer", nullable=False, is_primary_key=True, comment="主键"),
            Column("dev_code", "varchar(32)", nullable=False, comment="设备编号"),
            Column("model", "varchar(64)", nullable=True, comment="型号"),
            Column("status", "varchar(16)", nullable=False, comment="状态：在用/维修/报废"),
            Column("workshop_id", "integer", nullable=True, comment="所属车间"),
        ),
        sample_rows=(
            (1, "SN-001", "X-200", "在用", 10),
            (2, "SN-002", "X-200", "维修", 10),
            (3, "SN-003", "C-50", "在用", 11),
        ),
    ),
    Table(
        name="t_device_info",
        comment="设备扩展信息（历史系统，字段与 t_dev 部分重叠）",
        columns=(
            Column("device_no", "varchar(32)", nullable=False, is_primary_key=True),
            Column("model", "varchar(64)", nullable=True),
            Column("fw_ver", "varchar(16)", nullable=True, comment="固件版本"),
            Column("online_status", "tinyint", nullable=True, comment="0 离线 1 在线"),
        ),
        sample_rows=(
            ("SN-001", "X-200", "1.4.2", 1),
            ("SN-004", "C-50", "1.3.9", 0),
        ),
    ),
    Table(
        name="dev_backup",
        comment="设备备份表（疑似冗余）",
        columns=(
            Column("id", "integer", nullable=False, is_primary_key=True),
            Column("dev_code", "varchar(32)", nullable=False),
            Column("model", "varchar(64)", nullable=True),
        ),
        sample_rows=(
            (101, "SN-001", "X-200"),
            (102, "SN-002", "X-200"),
        ),
    ),
    Table(
        name="t_workshop",
        comment="车间",
        columns=(
            Column("workshop_id", "integer", nullable=False, is_primary_key=True),
            Column("workshop_name", "varchar(64)", nullable=False),
            Column("manager_name", "varchar(32)", nullable=True, comment="负责人姓名"),
        ),
        sample_rows=((10, "一号车间", "张伟"), (11, "二号车间", "李娜")),
    ),
    Table(
        name="t_ticket",
        comment="维修工单",
        columns=(
            Column("ticket_id", "varchar(32)", nullable=False, is_primary_key=True),
            Column("dev_id", "integer", nullable=True, comment="关联设备"),
            Column("title", "varchar(200)", nullable=True),
            Column("status", "varchar(16)", nullable=False, comment="状态"),
        ),
        sample_rows=(
            ("T-0001", 1, "主轴异响", "处理中"),
            ("T-0002", 2, "换刀机构故障", "已关闭"),
        ),
    ),
)


def sample_schema() -> Schema:
    return Schema(tables=_TABLES, dialect="fixture")
