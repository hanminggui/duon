# Duon 协议规范：确定性输出与 Canonical JSON 算法 (v1)

> 目的：实现“同一份文档在相同配置下多次处理结果在字节级别完全一致”，解决模型生成抖动、键名无序以及数组乱序问题，提供可供外部校验的不可篡改 `result_hash`。

---

## 1. 核心确定性公式

任意请求的最终确定性结果哈希 `result_hash` 计算方式如下：

```text
result_hash = SHA256(
    Canonicalize(data) +
    Canonicalize(evidence) +
    pipeline_version +
    model_version
)
```

---

## 2. Canonical JSON 规范化规则

在生成响应或计算哈希之前，必须对内部数据结构执行以下规整化流程：

### 2.1 键名排序 (Key Ordering)
- 所有 JSON Object 的键必须按 UTF-8 字节序（ASCII 字典序）进行升序排序。
- 嵌套子对象递归执行相同排序规则。

### 2.2 数值标准化 (Number Normalization)
- 整数不得带有小数位（如 `1600`，不得输出为 `1600.0`）。
- 浮点数采用规范的小数表示，去除末尾无意义的零（如 `16.50` 规整为 `16.5`）。
- 禁止使用科学计数法（例如 `1e6` 必须展开为 `1000000`）。
- 经计算的坐标统一保留 2 位小数四舍五入（`round(val, 2)`）。

### 2.3 文本与字符串规整 (String Canonicalization)
- 统一执行 Unicode NFC (Normalization Form C) 规整，消除不同系统下变音符号与组合字符的字节差异。
- 文本前后空格自动 `trim()`，内部连续换行规整为单一 `\n`。
- 日期格式强制规范为 ISO-8601（如 `YYYY-MM-DD` 或 `YYYY-MM-DDTHH:MM:SSZ`）。

### 2.4 数组排序策略 (Array Ordering Policy)
由于 LLM 生成列表项（如联系人列表、条款列表）时顺序可能随机波动，必须采用确定性规则进行重排：

1. **带位置证据的实体列表**：
   - 优先依据该元素绑定的第一个 Evidence 的位置排序：
     `page_number ASC` -> `bbox[1] ASC (Top-to-Bottom, y0)` -> `bbox[0] ASC (Left-to-Right, x0)`。
2. **纯值标量列表（如字符串标签数组）**：
   - 按字符串字典序升序排序。
3. **严格保留语序的段落/文本流**：
   - 仅对标记为 `ordered: true` 的 Schema 字段保持原始文档抽取顺序，不得随意打乱。

---

## 3. Evidence 引用与指针规范 (RFC 6901)

- 证据键必须严格遵循 **RFC 6901 JSON Pointer** 标准：
  - 根级字段：`/party_a`
  - 嵌套对象：`/contract/amount`
  - 数组元素：`/items/0/price`
- 绑定的 EvidenceItem 结构：
  ```json
  {
    "page": 1,
    "item_ids": ["p1_i0004", "p1_i0005"],
    "bbox": [120.50, 240.10, 310.80, 255.40],
    "quote": "总金额为人民币 1600 元",
    "confidence": 0.99
  }
  ```
- 若模型返回的 `item_ids` 在 Document IR 中不存在，校验器必须将其剥离并判定为无效幻觉引用，记录在 `validation.errors` 中。

---

## 4. 坐标系标准 (Top-Left Absolute Coordinate)

- 坐标定义：`[x0, y0, x1, y1]`
- 原点位置：**左上角 (Top-Left)** 为 `(0, 0)`。
- 轴方向：
  - X 轴向右递增：$x_0$ 为左边界，$x_1$ 为右边界（$x_0 \le x_1$）。
  - Y 轴向下递增：$y_0$ 为上边界，$y_1$ 为下边界（$y_0 \le y_1$）。
- 页面元数据：配合 `Page.width` 和 `Page.height` 使用，前端渲染高亮蒙层时可按比例直接换算为屏幕视口像素。
