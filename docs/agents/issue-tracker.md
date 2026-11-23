# 本地 Markdown issue tracker

## 规格和工单

- 每个功能使用 `.scratch/<feature-slug>/`，规格写在 `spec.md`。
- 每张实现工单单独写在 `issues/<NN>-<slug>.md`，编号从 `01` 开始。
- 发布时在对应功能目录创建 Markdown 文件。读取时按用户提供的路径或工单编号定位文件。
- 用文件顶部附近的 `Status:` 行标记状态，取值见[分流标签](triage-labels.md)。评论追加到文件底部的 `## Comments`。

## Wayfinder

- 计划写在 `.scratch/<effort>/map.md`，子工单按上面的工单约定存放。
- 用 `Type:` 标记子工单类型，取值为 `research`、`prototype`、`grilling` 或 `task`。用 `Blocked by:` 列出依赖的工单编号。
- 按编号选择首个开放、未领取且依赖均已解决的工单。开始前保存 `Status: claimed`。
- 完成时在 `## Answer` 记录答案，设置 `Status: resolved`，再将摘要和工单链接补入 `map.md` 的已定事项。
