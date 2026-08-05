# Security Policy

## 报告安全漏洞

请勿在公开渠道提交安全漏洞细节。请通过以下任一渠道私密报告：

- 私有漏洞报告：联系项目维护者（见 GitHub 仓库联系方式）
- 邮件：security@metria.example.com

预期响应时间：3 个工作日内确认；修复时间取决于严重程度与影响面。

## 安全边界

Metria 采用零侵入采集，系统边界如下：

- **Agent**：只读挂载客户端目录；不修改客户端配置/日志/数据库；不注入代理或 eBPF；
  不使用 Host Network / Host PID / Privileged / Docker Socket。
- **Hub**：默认单 Admin；Collector 凭据仅存哈希；上传需鉴权与幂等校验。
- **隐私**：默认 `content_mode=metadata`；不上传完整绝对路径、Git Remote、环境变量、
  API Key、Authorization、Cookie、SSH 私钥、数据库连接串；日志不输出 Token 或 Secret。

## 依赖与数据

- 外部价格目录（OpenRouter / LiteLLM）数据仅作参考，失败时继续使用最后有效快照。
- 流量为估算值，不等同于网卡或账单流量。

## 报告内容模板

请提供：

1. 影响组件（hub/agent/adapter/web）
2. 复现步骤与最小复现
3. 预期行为与实际行为
4. 环境信息（版本、部署方式、架构）
5. 如涉及敏感信息，请脱敏后提交
