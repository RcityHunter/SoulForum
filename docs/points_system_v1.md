# SoulForum 第一版积分系统方案（V1）

> 目标：在 SoulForum 当前 Rust + Axum + SurrealDB + Dioxus 架构下，设计一个可落地、可分期实施的积分系统。路线明确采用 **Karma + Merit 双轨**，**Trust 不进入第一版规则层**，仅保留未来扩展位。

## 1. 先给结论

### 1.1 双轨职责分工

- **Karma**：代表“社区活跃与内容被认可的广义贡献值”。
  - 适合自动累计。
  - 主要用于：排序、展示、基础 rank、轻量权限门槛、活动运营。
  - 特点：可增长、可回收、可有衰减/上限、允许带一定噪音。
- **Merit**：代表“高质量贡献的人工/半人工背书”。
  - 适合稀缺发放。
  - 主要用于：更高 rank、提名、申请更高权限、抗刷的核心信用锚。
  - 特点：稀缺、可审计、不应靠纯机器大量发放。
- **Trust**：第一版不做决策变量。
  - 当前前端已存在 `frontend/src/components/points.rs` 的 `trust_level` 占位字段，但后端并未实现真实规则。
  - V1 建议把它视为 **UI 预留字段**，不参与任何权限判断，避免把“声誉积分”和“风控信任”混成一层。

### 1.2 V1 的核心原则

1. **先让 Karma 跑起来，再把 Merit 做稀缺且可控。**
2. **权限与 Merit 绑定只做少量关键点，不大面积绑死。**
3. **防刷优先于绝对精细。** V1 不追求学术级反作弊，追求“刷子成本明显上升，误伤可接受”。
4. **尽量复用现有对象：users / topics / posts / notifications / action_logs / membergroups / permissions。**
5. **先做事件账本，再做复杂策略。** 否则后续无法复盘和调参。

---

## 2. 为什么这套方案贴合 SoulForum 当前架构

基于仓库现状，V1 方案不是凭空设计，而是尽量贴现有模型：

### 2.1 当前已经具备的基础

- 后端主线是 **Axum + SurrealDB**。
- 已有核心内容对象：`boards`、`topics`、`posts`、`users`。
- 已有权限体系骨架：
  - `users.role`
  - `users.permissions`
  - `membergroups`
  - `board_access`
  - `board_permissions`
- 已有风控/治理基础：
  - 限流：`src/api/guards.rs` 中已存在接口级 rate limit。
  - 封禁：`ban_rules` / `ban_logs` / `manage_bans`。
  - 审计：`action_logs`。
- 已有通知基础：`notifications`、`tasks.rs` 中已有 like 通知偏好逻辑雏形。
- 前端已存在积分 UI 占位：`frontend/src/components/points.rs`，字段为 `karma / merit / trust_level / backend_ready`。

### 2.2 当前缺口

- 还没有真正的点赞/认可落库模型。
- 还没有积分事件账本。
- `users` 表没有 Karma / Merit 聚合字段。
- Rank 与 membergroup/permissions 的自动联动尚未建立。

所以 V1 最合适的做法不是直接堆复杂规则，而是：

1. **补一层 points event ledger（积分事件账本）**；
2. **给 users 增加可读聚合字段**；
3. **先做最少事件来源**；
4. **再做 rank / 权限联动**。

---

## 3. V1 术语定义

## 3.1 Karma

Karma 是用户在社区中的“公共贡献热度值”，用于反映：

- 发帖/回帖等基础参与
- 内容被他人点赞/认可
- 帖子质量和持续活跃
- 是否长期正向参与，而不是一次性刷动作

Karma 适合被展示在：

- 用户卡片
- 主题页作者信息
- 排行榜
- 活跃榜/贡献榜
- 轻量门槛（例如解锁签名、创建更多内容、弱运营权限）

## 3.2 Merit

Merit 是社区对“高质量、可信、值得鼓励”的**稀缺认可**。

它不是活跃度，不应该靠灌水得到。Merit 更像：

- 精华内容奖励
- 管理员/版主授予
- 活动获奖
- 被已有高贡献成员有限额度转赠（放到 Phase 2）

Merit 适合用于：

- 高 rank 晋升门槛
- 申请 moderator / 特殊身份的参考项
- 精华作者标识
- 社区治理中的“质量信用”参考

## 3.3 Rank

Rank 是面向用户展示和局部权限映射的“等级层”。

V1 建议 Rank 本身不是独立复杂系统，而是由 **Karma + Merit 阈值** 计算得出。Rank 可以映射到：

- 前端展示称号
- membergroup（展示型/帖子数型组）
- 少量额外权限

---

## 4. V1 核心规则

## 4.1 Karma 获取规则（建议首版）

先只接 4 类事件，足够启动：

### A. 创建主题
- +3 Karma
- 触发条件：主题创建成功，且未被系统/风控标记为垃圾
- 每日计分上限：最多记前 10 个主题（即每日最多 +30）

### B. 发表回复
- +1 Karma
- 触发条件：回复创建成功
- 每日计分上限：最多记前 20 条回复（即每日最多 +20）

### C. 获得点赞 / Like
- 主题首帖或回复每收到 1 个有效点赞：作者 +2 Karma
- 点赞者自己不加 Karma
- 单个内容的点赞 Karma 上限：建议 +20
- 同一对用户（A 给 B）每日可产生的 Karma 次数：建议最多 3 次

### D. 内容被设为精选 / 推荐（人工）
- +20 Karma
- 由管理员/版主触发
- 该动作必须写审计日志

> 为什么首版不把“登录、浏览、收藏、私信、签到”纳入 Karma？
>
> 因为这些动作更容易空转，价值密度低，而且 SoulForum 当前已有内容主流程（topic/post）最稳定，先围绕内容做最合理。

## 4.2 Karma 扣减规则（建议首版）

### A. 内容被删除 / 软删除 / 判定违规
- 若主题/回复被删除且认定违规：回收该内容带来的基础 Karma
- 若该内容曾带来点赞 Karma，也一并回收对应部分
- 需要保留事件账本，不能直接“静默改余额”

### B. 点赞被撤销
- 作者对应 -2 Karma
- 仅回收该点赞产生的 Karma，不影响其他来源

### C. 封禁处罚（可选，首版建议保守）
- 被 ban 仅限制行为，不直接清空 Karma
- 严重违规可由管理员手动扣 Karma（写 audit）
- 不建议 V1 自动“ban = 大额扣分”，避免误封造成大面积数据修复

## 4.3 Merit 获取规则（建议首版）

V1 的 Merit 必须严格收敛，只允许以下来源：

### A. 管理员/版主授予
- 用途：奖励高质量内容、社区帮助、活动贡献
- 单次建议：+1 / +2 / +5
- 必填理由（reason）
- 必写 `action_logs`

### B. 内容加精/推荐（可配置二选一）
建议二选一，不要两套都上：

- **方案 A：加精即 Merit**
  - 精选一次 +1 Merit
  - 简单直观
- **方案 B：加精只给 Karma，Merit 必须人工发**
  - 更稳，更不容易泛滥

**我更建议 V1 用方案 B。**
原因：SoulForum 第一版治理动作还不多，先把 Merit 维持成真正稀缺资产，后面再开放半自动来源。

## 4.4 Merit 使用/限制规则

- V1 **不支持消费 Merit**。
- V1 **不支持成员之间互转 Merit**。
- V1 **不支持自动批量发 Merit**。
- V1 **不建议允许普通用户发 Merit**。

这样可以最大程度避免第一版把 Merit 做废。

---

## 5. 点赞 / 认可机制建议

因为 Karma 很大一部分来自“被认可”，所以 V1 最值得补的不是复杂积分页，而是 **最小可用点赞系统**。

## 5.1 建议新增 `likes` 表

建议新增 Surreal collection：`likes`

字段建议：

- `id`
- `target_type`: `topic` / `post`
- `target_id`
- `target_author_id`
- `actor_id`
- `created_at`
- `status`: `active` / `revoked`
- `source`: `user` / `mod_reward`（可选）

索引建议：

- `(target_type, target_id)`
- `(actor_id, target_type, target_id)` 唯一，防止重复点赞
- `(target_author_id, created_at)` 用于反刷统计

## 5.2 点赞有效性判定

一个点赞要产生 Karma，至少满足：

- 点赞者不是内容作者本人
- 目标内容处于正常可见状态
- 点赞者账号不是 guest
- 点赞者账号通过最小门槛（见 7. 防刷）
- 该点赞未被撤销

---

## 6. Rank 与权限绑定建议

V1 不建议做太多等级，否则会把积分系统变成权限系统，难以控制。建议只做 **4 档 rank**，且以展示为主、权限为辅。

## 6.1 建议 Rank

### Rank 0：Newcomer
- 条件：默认注册用户
- 展示：新成员
- 权限：现有基础权限即可（`post_new` / `post_reply_any` 等现有默认权限）

### Rank 1：Member
- 条件：Karma >= 20
- 展示：正式成员
- 建议权限：
  - 可使用签名（若已有相关权限位）
  - 更高的日发帖额度
  - 可点赞

### Rank 2：Contributor
- 条件：Karma >= 100 且 Merit >= 1
- 展示：贡献者
- 建议权限：
  - 可申请内容推荐/参与活动报名
  - 更高的个人资料展示项权限
  - 可进入部分半公开讨论区（如果后续需要）

### Rank 3：Core
- 条件：Karma >= 300 且 Merit >= 3
- 展示：核心成员
- 建议权限：
  - 仅作为 moderator 候选池参考，不自动给管理权限
  - 可获得明显徽章/UI 标识

> 关键建议：**Moderator 不要由 rank 自动晋升。**
>
> `moderate_forum`、`manage_bans`、`manage_boards` 这类权限仍应保持人工授予。

## 6.2 Rank 映射到现有 membergroups 的建议

SoulForum 当前已有：

- `membergroups`
- `primary_group`
- `additional_groups`
- `min_posts`

但当前骨架更偏传统论坛，不是现成的“积分等级系统”。

V1 建议：

- **不要直接复用 `min_posts` 作为 Karma 判级依据**。
- 新增一组“展示型组”（post group / badge group 均可，但建议独立）用于 rank 显示：
  - `Newcomer`
  - `Member`
  - `Contributor`
  - `Core`
- 由后台任务或登录时刷新 `primary_group / additional_groups` 中的展示组。

更稳的做法是：

- `primary_group` 保留真实身份组（admin / mod / 普通组）
- `additional_groups` 增加 rank 展示组

这样不会污染现有管理权限逻辑。

---

## 7. 防刷思路（V1 必须有，但别做过重）

第一版最容易被刷烂的是 Karma，不是 Merit。

## 7.1 账号门槛

只有满足以下条件的账号，其点赞/互动才能产生 Karma：

- 注册超过 24 小时；或
- 已发过至少 1 个通过审核的主题/回复；或
- Karma >= 5

这能有效压低“新注册小号互赞”的收益。

## 7.2 自赞禁止

- 自己不能给自己点赞。
- 同一账号对同一目标只能有 1 个 active like。

## 7.3 配对限额

- 同一 `actor -> author` 在 24h 内，最多让对方获得 3 次点赞 Karma。
- 超出的点赞可保留“点赞状态”，但不再产出 Karma。

这条非常关键，能显著抑制互刷小团体。

## 7.4 日上限

建议为 Karma 设置日上限：

- 发主题获得 Karma：每日最多 30
- 回复获得 Karma：每日最多 20
- 被点赞获得 Karma：每日最多 40
- 人工推荐获得 Karma：不受日常上限，但必须审计

## 7.5 撤销可回滚

- 点赞撤销、内容删除、违规处理，都必须能回滚 Karma。
- 因此 V1 必须保留**事件账本**，不能只在 `users.karma` 上做加减。

## 7.6 Merit 防刷

因为 V1 的 Merit 只允许管理侧发放，所以核心策略是：

- 发放需要权限（建议 `grant_merit` 或复用管理权限）
- 必填理由
- 必写 `action_logs`
- 后台可按人查看最近发放记录
- 单个管理员每日发放总量建议设软上限（如 20）并告警，不必直接阻断

---

## 8. 数据模型建议

## 8.1 `users` 聚合字段

建议在 `users` 上补充：

- `karma`: `int`，当前总 Karma
- `merit`: `int`，当前总 Merit
- `rank_key`: `string`，如 `newcomer/member/contributor/core`
- `rank_updated_at`: `datetime`
- `points_last_calculated_at`: `datetime`（可选）

说明：

- `karma` / `merit` 是**读优化聚合值**。
- 真正可信的数据来源是事件账本。

## 8.2 新增 `point_events`

建议新增 Surreal collection：`point_events`

字段建议：

- `id`
- `user_id`
- `point_type`: `karma` / `merit`
- `delta`: `int`（正负都支持）
- `source_type`:
  - `topic_created`
  - `reply_created`
  - `like_received`
  - `like_revoked`
  - `featured_content`
  - `manual_reward`
  - `manual_penalty`
  - `content_deleted_revert`
- `source_id`: 关联对象 id（topic/post/like/action）
- `source_actor_id`: 触发人（如点赞者/管理员）
- `reason`: 文本，可空
- `status`: `active` / `reverted`
- `created_at`
- `reverted_at`
- `extra`: json（可选）

索引建议：

- `(user_id, point_type, created_at)`
- `(source_type, source_id)`
- `(source_actor_id, created_at)`

## 8.3 可选新增 `merit_grants`

如果希望 Merit 审计更清晰，可单独建表：`merit_grants`

字段：

- `id`
- `target_user_id`
- `granted_by`
- `amount`
- `reason`
- `related_content_type`
- `related_content_id`
- `created_at`
- `revoked_at`

但如果想控制复杂度，**V1 也可以先不单独建表，直接用 `point_events + action_logs` 即可。**

---

## 9. 后端实现建议

## 9.1 事件驱动，而不是到处散写 users.karma

建议引入统一服务：

- `PointsService::award_karma(...)`
- `PointsService::award_merit(...)`
- `PointsService::revert_event(...)`
- `PointsService::refresh_rank(...)`

主题、回复、点赞、精选这些业务动作只发“积分事件”，不要自行更新余额。

## 9.2 首批接入点

### 主题创建成功后
- 在 topic create handler 成功后追加 `topic_created` Karma 事件

### 回复创建成功后
- 在 reply create handler 成功后追加 `reply_created` Karma 事件

### 点赞成功后
- 新增 like API
- 校验通过后：
  - 写 `likes`
  - 给作者追加 `like_received` Karma 事件
  - 可发通知

### 点赞撤销后
- 追加 `like_revoked` / revert 事件

### 后台人工奖励 Merit
- 新增 admin route：`/admin/points/merit/grant`
- 权限：建议 admin/mod 可用，但最好单独权限位
- 写 `point_events` + `action_logs`

## 9.3 Rank 刷新策略

V1 建议不要做复杂异步任务系统，直接用下面方案：

- 每次 Karma / Merit 变化后，立即重算目标用户的 `rank_key`
- 登录时如果发现字段缺失，也可补算一次
- 后续再加 nightly reconcile job

---

## 10. 前端展示建议

前端已有 `frontend/src/components/points.rs`。

当前状态：

- `karma` / `merit` 已有 UI 字段
- `trust_level` 只是占位
- `backend_ready` 已有“preview / live”切换位

## 10.1 V1 前端建议

### 保留字段，但改语义
- `karma`: 接真实值
- `merit`: 接真实值
- `trust_level`: 不展示真实 Trust，可改成：
  - 隐藏该字段；或
  - 临时展示 rank 文案；或
  - 显示 `Trust: Not in V1`

我更建议：**把 trust 区块改成 rank 区块**，避免误导。

### 典型展示位
- 用户资料页
- 主题页作者卡片
- 首页侧边栏贡献榜（后续）
- 后台用户详情页

### 后台管理页
建议新增：
- 用户当前 Karma / Merit / Rank
- 最近积分事件列表
- Merit 发放入口

---

## 11. 权限绑定建议（细化）

V1 只绑下面几类，够用：

### 11.1 与 Karma 绑定的轻权限

- `likes_like`：建议要求 Rank >= Member，避免新号直接刷赞
- 更高频操作额度：如更高日发帖量/更宽草稿/附件额度
- 个性化展示项：如签名、资料扩展字段

### 11.2 与 Merit 绑定的质量权限

- 申请精选作者 / 活动嘉宾 / 社区志愿者的参考条件
- 进入候选 moderator 名单

### 11.3 不应绑定的权限

以下权限不要自动由积分开放：

- `manage_boards`
- `manage_bans`
- `manage_permissions`
- `moderate_forum`
- 任何可造成治理事故的管理权限

---

## 12. 运营口径建议

为了避免用户把 Karma 和 Merit 搞混，产品文案要非常清楚：

- **Karma = 活跃贡献值**
- **Merit = 高质量认可值**

建议对外文案示例：

- Karma：通过发帖、回复、获得点赞等行为逐步增长
- Merit：由社区管理团队授予，用于认可真正高质量贡献

这样能天然减少“为什么我发了很多水贴却没 Merit”的争议。

---

## 13. 分阶段实施建议

## Phase 1：可上线的最小版本

目标：先跑通双轨基础，不追求完整生态。

### 范围
- `users` 增加 `karma / merit / rank_key`
- 新增 `point_events`
- 主题创建 -> +Karma
- 回复创建 -> +Karma
- 后台人工发 Merit
- 后台可查看用户积分
- 前端用户卡片显示 Karma / Merit / Rank
- 不做 Trust

### 价值
- 立刻形成“贡献可见”反馈
- 给社区运营一个基本抓手
- 为后续点赞、精选、排行榜打地基

### 风险控制
- 暂不开放用户之间转赠
- 暂不开放复杂自动奖励
- 暂不做公开排行榜也可以，先内部验证数据质量

## Phase 2：把认可链路做完整

目标：让 Karma 真正由社区反馈驱动。

### 范围
- 新增 `likes`
- 点赞/撤销点赞 -> Karma 增减
- 通知联动
- 反刷规则上线：账号门槛、配对限额、日上限
- 后台积分事件审计页
- 贡献榜/热门作者榜

### 可选增强
- 精选内容 + Karma
- 精选内容 + Merit（如果社区治理已成熟）

## Phase 3：治理与生态扩展

目标：把积分从“展示”升级为“社区秩序工具”。

### 范围
- 更细的 rank 体系
- 按版块加权 Karma
- Merit 提名/审批流
- 运营活动积分任务
- 周/月衰减或活跃度维度（只对 Karma，不碰 Merit）
- 反作弊规则中心与可视化报表
- 若后续真的需要，再单独设计 Trust 风控层

### 强提醒
- **Trust 必须单独设计，不能直接等于 Karma 或 Merit。**
- Trust 更像账号可信度/风险层，应考虑注册时长、设备/IP、违规史、申诉、人工审核等，不该混在 V1。

---

## 14. 推荐的 V1 最小 API / 管理接口

## 14.1 用户侧
- `GET /api/me/points`
- `GET /api/users/:id/points`

返回建议：

```json
{
  "karma": 128,
  "merit": 2,
  "rank_key": "contributor",
  "rank_name": "Contributor",
  "backend_ready": true
}
```

## 14.2 管理侧
- `POST /admin/points/merit/grant`
- `POST /admin/points/karma/adjust`
- `GET /admin/points/users/:id/events`

其中：
- `karma/adjust` 用于修数和处罚
- 必须审计

---

## 15. 推荐的字段与规则取舍

## 15.1 第一版建议做
- Karma
- Merit
- Rank
- 点积分事件账本
- 后台人工 Merit
- 少量权限映射

## 15.2 第一版明确不做
- Trust 真规则
- 用户互赠 Merit
- 连续签到积分
- 浏览积分
- 登录积分
- 自动 moderator 晋升
- 超复杂权重公式
- 黑盒风控分

这是我对 SoulForum V1 的明确判断：**宁可简单但可信，也不要功能全但几周内被刷烂。**

---

## 16. 风险点与规避

## 16.1 风险：Karma 太容易刷
规避：
- 新号门槛
- 自赞禁止
- 配对限额
- 日上限
- 可回滚账本

## 16.2 风险：Merit 发滥
规避：
- 只允许管理侧发
- 理由必填
- 审计日志必写
- 后台可追溯谁发给了谁

## 16.3 风险：Rank 直接影响管理权限导致事故
规避：
- Rank 只影响展示和轻权限
- 管理权限保持人工授予

## 16.4 风险：后续规则变更导致历史数据无法修复
规避：
- 一开始就做 `point_events`
- 聚合值只是缓存，不是唯一真相

---

## 17. 最终推荐落地顺序

### 第 1 步
补数据结构：
- `users.karma`
- `users.merit`
- `users.rank_key`
- `point_events`

### 第 2 步
接入最稳定事件：
- topic create
- reply create
- admin merit grant

### 第 3 步
前端把积分卡片接真数据：
- 把 `trust_level` 从文案上降级/隐藏
- 展示 Karma / Merit / Rank

### 第 4 步
再补点赞系统：
- `likes`
- 点赞通知
- Karma from likes
- 反刷规则

### 第 5 步
最后做榜单和更多运营能力

---

## 18. 一句结论

如果 SoulForum 第一版要做一个 **能上线、能解释、能防刷、还能给后续治理留空间** 的积分系统，最稳的路线就是：

- **Karma 负责“活跃贡献”**
- **Merit 负责“高质量背书”**
- **Rank 只做展示和轻权限映射**
- **Trust 暂不进入 V1**
- **先做账本，再做玩法**

这条路不花哨，但最不容易在两个月后推倒重来。
