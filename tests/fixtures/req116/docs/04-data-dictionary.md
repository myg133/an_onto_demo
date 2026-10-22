# 数据字典（节选）

本文件是业务系统与数据仓库之间的字段约定，写给 FDE 与数据分析同学看。

## 客户表 customer

| 字段 | 类型 | 业务含义 |
| --- | --- | --- |
| customer_id | bigint | 客户主键，平台在注册时生成 |
| mobile | varchar | 客户手机号，注册时校验 |
| level | varchar | 会员档位，取值为 basic / pro |

客户注册时，平台向客户手机号发送验证码。客户通过验证后，平台创建客户记录。

## 订单表 orders

| 字段 | 类型 | 业务含义 |
| --- | --- | --- |
| order_no | varchar | 订单号，平台在下单时生成 |
| customer_id | bigint | 下单的客户 |
| amount | numeric | 订单金额（元） |
| status | varchar | 订单状态 |

订单与客户是 N:1：一个客户可以有多笔订单，一笔订单只属于一个客户。

## 支付表 payment

支付表记录每一次扣费。一次扣费对应一笔订单；一笔订单可能有多条扣费记录
（例如先支付失败、后补支付成功）。支付渠道在扣费成功后回传渠道流水号（channel_no）。

## 术语对照

- 下单 = place order
- 扣费 = charge
- 退款 = refund
- 订阅 = subscribe
