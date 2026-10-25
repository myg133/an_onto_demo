# -*- coding: utf-8 -*-
"""生成 REQ-144 的**合成**夹具（⛔ 不含任何真实数据）。

形状照腾讯云官方文档（DescribeInstances / DescribeAlarmHistories 的 Response 结构），
值是造的：ins-0001..ins-0250、web-0001..、两个可用区、两种状态。

每份产物顶层带一个 `_comment` 数组 —— 它**不是** API 的返回字段，而是本脚本写的溯源说明
（真实调用不会出现它，所以任何 mapping 都⛔ 不许引用 `_comment`）。
它同时是「这件夹具是脚本产物、不是手改的」的凭据：手改过 ⇒ 重跑本脚本后与库里的版本 sha 不同。

跑法（**幂等**：连跑两次 sha 相同）： python fixtures/gen_access_fixtures.py
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
        "_comment": [
            "REQ-144 离线夹具（合成数据，⛔ 不含任何真实 ID / 凭据）。",
            "本字段不是 API 的返回：它是 fixtures/gen_access_fixtures.py 写的溯源说明（真实调用不会出现它）。",
            "形状照腾讯云 DescribeInstances 的 Response：TotalCount / InstanceSet / RequestId。",
            "%d 行 / paging.max_limit=100 ⇒ 3 页（100/100/50）。" % n,
            "InstanceState=STOPPED 83 行（i%%3==0 ⇒ i = 3,6,…,249），其余 %d 行 RUNNING。" % (n - 83),
            "Zone 交替：奇数行 ap-guangzhou-2（125 行）、偶数行 ap-guangzhou-3（125 行）。",
        ],
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
        "_comment": [
            "REQ-144 离线夹具（合成数据，⛔ 不含任何真实 ID / 凭据）。",
            "本字段不是 API 的返回：它是 fixtures/gen_access_fixtures.py 写的溯源说明（真实调用不会出现它）。",
            "形状照腾讯云 DescribeAlarmHistories 的 Response：TotalCount / AlarmHistorySet / RequestId。",
            "3 行；AlarmObject = 实例 ID（CVM 的告警对象就是实例）⇒ 关系 alarm__cvm_instance 的 join 列。",
            "AlarmStatus：ALARM 2 行（alarm-0001/0003）、OK 1 行（alarm-0002）。",
        ],
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
