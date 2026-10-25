# -*- coding: utf-8 -*-
"""生成 REQ-144 的**合成**夹具（⛔ 不含任何真实数据）。

形状照腾讯云官方文档（DescribeInstances / DescribeAlarmHistories 的 Response 结构），
值是造的：ins-0001..ins-0250、web-0001..、两个可用区、两种状态。
来源与口径写在 `fixtures/README.md`（夹具本身保持纯响应形状，⛔ 不夹注释字段）。

跑法： python fixtures/gen_access_fixtures.py
"""
import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "fixtures")

ZONES = ["ap-guangzhou-3", "ap-guangzhou-2"]


def describe_instances(n=250):
    rows = []
    for i in range(1, n + 1):
        rows.append({
            "InstanceId": "ins-%04d" % i,
            "InstanceName": "web-%04d" % i,
            "InstanceState": "STOPPED" if i % 3 == 0 else "RUNNING",
            "InstanceType": "S5.MEDIUM4" if i % 2 else "S5.LARGE8",
            "CPU": 4 if i % 2 else 8,
            "Memory": 8 if i % 2 else 16,
            "Placement": {
                "Zone": ZONES[i % 2],
                "ProjectId": 0,
            },
            "CreatedTime": "2026-%02d-%02dT08:00:00Z" % (1 + i % 9, 1 + i % 27),
            "InstanceChargeType": "PREPAID" if i % 4 == 0 else "POSTPAID_BY_HOUR",
            "Tags": [{"Key": "env", "Value": "prod" if i % 5 else "dev"}],
        })
    return {
        "Response": {
            "TotalCount": n,
            "InstanceSet": rows,
            "RequestId": "00000000-0000-4000-8000-000000000000",
        },
    }


def alarm_histories():
    rows = [
        {
            "AlarmId": "alarm-0001",
            "MonitorType": "MT_QCE",
            "AlarmObject": "ins-0002",
            "AlarmStatus": "ALARM",
            "AlarmType": "metric",
            "StartTime": 1760000000,
            "EndTime": None,
            "ProjectId": 0,
            "Namespace": "QCE/CVM",
        },
        {
            "AlarmId": "alarm-0002",
            "MonitorType": "MT_QCE",
            "AlarmObject": "ins-0004",
            "AlarmStatus": "OK",
            "AlarmType": "metric",
            "StartTime": 1760003600,
            "EndTime": 1760007200,
            "ProjectId": 0,
            "Namespace": "QCE/CVM",
        },
        {
            "AlarmId": "alarm-0003",
            "MonitorType": "MT_QCE",
            "AlarmObject": "ins-0006",
            "AlarmStatus": "ALARM",
            "AlarmType": "metric",
            "StartTime": 1760010000,
            "EndTime": None,
            "ProjectId": 0,
            "Namespace": "QCE/CVM",
        },
    ]
    return {
        "Response": {
            "TotalCount": len(rows),
            "AlarmHistorySet": rows,
            "RequestId": "00000000-0000-4000-8000-000000000001",
        },
    }


def main():
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, "tencent-cvm-describe.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump(describe_instances(), f, ensure_ascii=False, indent=2)
        f.write("\n")
    with open(os.path.join(OUT, "tencent-monitor-alarm-histories.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump(alarm_histories(), f, ensure_ascii=False, indent=2)
        f.write("\n")
    print("ok")


if __name__ == "__main__":
    main()
