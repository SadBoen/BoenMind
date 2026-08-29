# ADR-0010:第三方中转网关作为模型 Provider

- 状态:Accepted(2026-08-30)
- 关联:基线 5.4(模型连接器也是 Provider)、4.6(Secret Store)、8.4(脱敏与信任分级)、
  ADR-0007(L0 启动豁免)、M4 规格 §5.8(模型调用豁免,M7 复议)、M7 规格 S1/S2

## 背景

M7 引入真实模型 Provider。用户提供的可用端点是第三方中转网关(NewAPI 形态,
OpenAI 兼容,`/v1/chat/completions`),非官方直连。网关运营方对经过的全部
提示词与输出内容具有事实上的可见性。

## 决策

1. **接受第三方网关作为 M7 的真实模型通道**,以 OpenAI 兼容协议接入
   (OpenAiHttpConnector),模型 `gpt-5.6-luna`。这是用户明示的供给选择,
   Runtime 不对其可用性/合规性作额外担保。
2. **信任边界记录**:经第三方网关传输的内容,视同对网关运营方可见。
   - 不改变 input_trust 语义:信任分级仍按内容来源链(基线 8.4),
     传输通道与内容信任无关;
   - 风险告知:用户不应经该通道发送高于自身容忍度的敏感数据;
     官方直连通道与端到端加密传输留待后续 ADR。
3. **密钥治理**:API key 只存 Secret Store(本地 FileSecretStore,gitignored);
   事件/日志/错误一律凭据脱敏(INV-5 既有纪律);仓库零明文,仅 example 模板。
4. **调用面收编**:模型调用自 M7 起过 Broker(M4 §5.8 豁免撤销),
   manifest `model.invoke`,审计照发——模型是能力,不是旁路(基线 5.4 兑现)。

## 后果

- 正面:真实 Provider 与外部能力同构(M7 通过条件第一句);密钥治理闭环;
  为 M8 多模型/官方直连预留可替换位。
- 代价:每 turn 增加一次查表与一条审计事件(性能影响由 perf 记录⑤实测留档);
  网关注入的系统提示计入 prompt tokens(观测到的 ~4.7k),成本核算需计入。
- 回退:移除环境变量装配即回到 MockConnector,合同与调用面不变。

## 条件与验收

1. M7 通过条件五句全部有测试钉住(M7-review 结算)。
2. INV-5 泄漏扫描覆盖 FileSecretStore(expose_for_scan)。
3. 实网验证留档:一次真实 chat completion 成功(测试 #[ignore],env 门控)。
