{{#if report.is_weekly}}
# 📊 {{repo.name}} 周报（{{report.start_date}} ~ {{report.end_date}}）

> 生成时间：{{generated_at}}
> 仓库数量：{{report.repo_count}}
> 分支：`{{repo.branch}}`
> 提交数量：{{report.commit_count}}，涉及文件：{{report.file_count}}

---

{{#if repos}}
## 关联仓库
{{#each repos}}
- `{{name}}`：{{path}}（{{branch}}）
{{/each}}

---
{{/if}}

## 一、本周计划（Weekly Plan）
{{#if summary.risks}}
{{#each summary.risks}}
- [ ] 跟进：{{this}}
{{/each}}
{{else}}
{{#if summary.modules}}
{{#each summary.modules}}
- [ ] 持续推进 `{{this}}` 相关工作
{{/each}}
{{else}}
- [ ] 补充本周工作计划
{{/if}}
{{/if}}

---

## 二、本周总结（Weekly Summary）
### 1. 重点成果
{{#if summary.highlights}}
{{#each summary.highlights}}
- {{this}}
{{/each}}
{{else}}
- 本周暂无匹配到的提交成果。
{{/if}}

### 2. 完成情况对比（计划 vs 实际）
| 计划事项 | 完成情况 | 备注 |
|----------|----------|------|
{{#if summary.modules}}
{{#each summary.modules}}
| 推进 `{{this}}` | 已有提交记录 | 详见下方提交概览 |
{{/each}}
{{else}}
| 待补充 | 无提交记录 | 可结合项目实际补充 |
{{/if}}

### 3. 问题与挑战
{{#if summary.risks}}
{{#each summary.risks}}
- 问题：{{this}}
  - 原因：来自提交信息中的风险关键词
  - 解决方案：建议继续跟进相关提交或补充人工说明
  {{/each}}
  {{else}}
- 本周未从提交信息中识别到显式问题。
{{/if}}

### 4. 风险与改进
{{#if summary.risks}}
{{#each summary.risks}}
- 风险点：{{this}}
- 改进措施：补充验证结果、完善文档或拆分后续任务
{{/each}}
{{else}}
- 风险点：暂无显式风险
- 改进措施：继续保持提交说明清晰，便于自动汇总
{{/if}}

---

## 三、下周计划（Next Week Plan）
{{#if summary.modules}}
{{#each summary.modules}}
- [ ] 继续推进 `{{this}}` 模块
{{/each}}
{{else}}
- [ ] 补充下周计划
{{/if}}

---

## 四、日报汇总（Daily Logs Summary）
> 说明：根据周内每日提交自动归纳

{{#each daily_logs}}
### {{label}}（{{date}}）
- 工作内容：{{items_display}}
- 问题/困难：{{risks_display}}
- 解决方案：{{solutions_display}}

{{/each}}

## 五、提交概览
{{#if commits}}
{{#each commits}}
### {{date}} · {{summary}}
- 来源仓库：`{{repo_name}}`
- 作者：{{author}} <{{email}}>
- 影响模块：{{modules_display}}
- 变更文件：{{files_display}}
{{#if body}}
- 备注：{{body}}
{{/if}}

{{/each}}
{{else}}
暂无提交概览。
{{/if}}

## 六、文档参考
{{#if docs}}
{{#each docs}}
### {{title}}
- 文档路径：`{{path}}`
- 摘要：{{excerpt}}

{{/each}}
{{else}}
- 未发现可用项目文档。
{{/if}}
{{/if}}

{{#if report.is_daily}}
# 📝 {{repo.name}} 日报（{{report.start_date}}）

> 生成时间：{{generated_at}}
> 仓库数量：{{report.repo_count}}
> 分支：`{{repo.branch}}`
> 提交数量：{{report.commit_count}}，涉及文件：{{report.file_count}}

## 📅 日期：{{report.start_date}}

### 一、今日工作内容
{{#if summary.highlights}}
{{#each summary.highlights}}
- {{this}}
{{/each}}
{{else}}
- 今日暂无匹配到的提交记录。
{{/if}}

### 二、完成情况
- 已完成：{{summary.modules_display}}
{{#if summary.risks}}
- 进行中：{{#each summary.risks}}{{this}}；{{/each}}
{{else}}
- 进行中：可继续围绕已变更模块补充后续计划
{{/if}}

### 三、问题与困难
{{#if summary.risks}}
{{#each summary.risks}}
- 问题描述：{{this}}
- 影响范围：需结合对应模块和业务场景补充
{{/each}}
{{else}}
- 暂未从提交信息中识别到显式问题。
{{/if}}

### 四、解决方案与进展
- 已采取措施：已完成相关 Git 提交与文件变更
- 当前进展：{{summary.modules_display}}
{{#if docs}}
- 文档核对：{{#each docs}}{{title}}；{{/each}}
{{/if}}

### 五、明日计划
{{#if summary.modules}}
{{#each summary.modules}}
- [ ] 继续推进 `{{this}}` 相关事项
{{/each}}
{{else}}
- [ ] 补充明日计划
{{/if}}

---

## 🔧 可选增强字段（按需使用）
{{#if summary.risks}}
- 风险预警：{{#each summary.risks}}{{this}}；{{/each}}
{{else}}
- 风险预警：暂无
{{/if}}
- 需协助事项：如需更细的业务表述，可在模板中追加人工补充
- 关键沟通记录：建议结合文档与 commit body 继续完善

## 参考提交
{{#if commits}}
{{#each commits}}
- `{{repo_name}}` / {{summary}}（{{files_display}}）
{{/each}}
{{else}}
- 暂无提交记录
{{/if}}

## 文档参考
{{#if docs}}
{{#each docs}}
- `{{path}}`：{{excerpt}}
{{/each}}
{{else}}
- 未发现可用项目文档。
{{/if}}
{{/if}}
