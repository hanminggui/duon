# Duon Canonical Document IR (中间表示) 开放标准规范 (v1.0)

> **标准状态**：正式规范 (Draft Standard v1.0)  
> **适用范围**：任何文档解析引擎（LiteParse, Docling, PyMuPDF, OCR 等）与语义理解/LLM 抽取引擎之间的事实互操作协议。

---

## 1. 规范背景与设计宗旨

### 1.1 核心目标：解析与语义的彻底解耦
长期以来，文档处理系统存在两大痛点：
1. **解析器强绑定**：业务提取逻辑深度依赖某一具体解析库（如特定版本的 LiteParse 或 PDFium）的原始私有 JSON 结构，解析器一升级，业务全崩塌。
2. **缺乏空间与事实锚点**：大模型往往只看纯文本，丢失物理位置，导致抽取出的字段无法溯源、无法做前端高亮，甚至模型自行编造坐标。

**Duon Document IR (Document Intermediate Representation)** 的设计初衷是建立一个**公开、中立、自包含、具有几何确定性**的“文档事实中间层标准”。关于 Document IR 相比直接透传解析器原始输出所带来的 5 大质的提升与性能优势，详见专论文档：[Document IR 设计哲学与效果提升原理](../../docs/document-ir-design-rationale.md)。

### 1.2 互操作愿景
- **场景 A（自带解析器）**：第三方可以使用 Python（Docling/PaddleOCR）、C++ 或 Go 实现自己的解析服务，只要输出符合本标准的 Document IR，就能直接调用 Duon 的 `POST /v1/extract` 与 `POST /v1/verify` 进行高精度抽取与核验。
- **场景 B（自带抽取器）**：调用方可以只使用 Duon 的 `POST /v1/parse` 生成标准 Document IR，然后由调用方自己的私有模型或规则引擎消费该 IR。

---

## 2. 规范定义词汇 (RFC 2119)
本规范中的关键字 **MUST（必须）**、**MUST NOT（严禁）**、**REQUIRED（要求）**、**SHALL（应当）**、**SHOULD（建议）**、**MAY（可选）** 均依据 RFC 2119 标准解释。

---

## 3. 空间几何与坐标系标准 (Spatial Model)

任何合规的 Document IR 生产者与消费者 **MUST** 遵循以下几何公理：

```text
(0,0) [Top-Left 原点] ─────────────► X 轴 (向右递增, 宽度: width)
 │
 │   [x0, y0] ┌─────────────┐
 │            │  TextItem   │
 │            └─────────────┘ [x1, y1]
 ▼
 Y 轴 (向下递增, 高度: height)
```

1. **原点位置 (Origin)**：
   - 原点 `(0, 0)` **MUST** 位于页面的**左上角 (Top-Left)**。
   - 若底层解析引擎（如原始 PDFium / PostScript）采用左下角原点，适配器 **MUST** 在转换为 IR 前完成 Y 轴翻转换算：
     $$y_{top} = \text{height} - y_{bottom}$$
2. **包围盒格式 (Bounding Box)**：
   - 包围盒 **MUST** 表示为长度为 4 的数值数组：`[x0, y0, x1, y1]`。
   - 几何单调性约束：**MUST** 满足 $x_0 \le x_1$ 且 $y_0 \le y_1$。
3. **坐标度量单位 (Units)**：
   - 坐标数值 **MUST** 采用标准点（Points，1 pt = 1/72 英寸，PDF 经典印刷点）。
   - 每页 **MUST** 明确给出 `width` 与 `height`（以相同单位度量），确保前端渲染器可无损计算归一化视口比例：
     $$\text{norm\_x} = \frac{x}{\text{width}}, \quad \text{norm\_y} = \frac{y}{\text{height}}$$

---

## 4. 全局唯一与确定性条目 ID 规范 (Identifier Norms)

为了保证大模型引用证据时的紧凑性与确定性，所有原子条目 **MUST** 具备全局稳定的语义标识符：

| 条目类型 | ID 命名正则表达式 | 命名范式 | 示例 | 约束与说明 |
| :--- | :--- | :--- | :--- | :--- |
| **文本条目 (TextItem)** | `^p[0-9]+_i[0-9]+$` | `p{page}_i{index:04}` | `p1_i0001` | 文档内全局唯一；递增索引，从 1 开始。 |
| **版面区块 (Block)** | `^p[0-9]+_b[0-9]+$` | `p{page}_b{index:04}` | `p1_b0002` | 表示段落、标题等聚合块。 |
| **表格 (Table)** | `^p[0-9]+_t[0-9]+$` | `p{page}_t{index:04}` | `p2_t0001` | 标识完整表格对象。 |
| **图像 (ImageItem)** | `^p[0-9]+_img[0-9]+$` | `p{page}_img{index:04}`| `p1_img0001`| 标识嵌入图或印章位置。 |

> **关键设计考量**：前缀固定为 `p{page}_` 让模型在生成 Token 时极其容易感知页码，同时极大地压低上下文 Token 消耗。

---

## 5. Document IR 数据结构全集 (Schema Specification)

### 5.1 根对象 (DocumentIR Root)
```json
{
  "document_id": "doc_e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  "source": {
    "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "mime_type": "application/pdf",
    "filename": "order.pdf",
    "size_bytes": 1048576
  },
  "parser": {
    "name": "liteparse",
    "version": "2.14.4",
    "config_hash": "a1b2c3d4e5f6"
  },
  "pages": [ ... ]
}
```
- `document_id` [String, **REQUIRED**]：文档身份标识，推荐为 `doc_{sha256}`。
- `source.sha256` [String(64), **REQUIRED**]：原文档内容的十六进制 SHA-256 哈希。
- `parser` [Object, **REQUIRED**]：标注产生此事实的解析器信息，便于审计与版本追溯。

---

### 5.2 页面对象 (Page)
```json
{
  "page_number": 1,
  "width": 595.32,
  "height": 841.92,
  "items": [ ... ],
  "blocks": [ ... ],
  "tables": [ ... ],
  "images": [ ... ]
}
```
- `page_number` [Integer, **REQUIRED**]：1-indexed 物理页码（首页为 1）。
- `width`, `height` [Number, **REQUIRED**]：页面点坐标宽高，必须严格大于 0。
- `items` [Array<TextItem>, **REQUIRED**]：页面内所有基本文本图元的有序列表。

---

### 5.3 基础图元 (TextItem) — 事实最小单元
```json
{
  "id": "p1_i0001",
  "type": "text",
  "text": "委托制作采购订单",
  "bbox": [210.50, 45.20, 385.00, 72.80],
  "confidence": 0.99
}
```
- `id` [String, **REQUIRED**]：符合 `p{page}_i{index}` 规则的稳定 ID。
- `text` [String, **REQUIRED**]：文字内容（经 Unicode NFC 规整）。
- `bbox` [Array<Number>(4), **REQUIRED**]：左上角 `[x0, y0, x1, y1]`。
- `confidence` [Number(0.0~1.0), **OPTIONAL**]：OCR 或字形提取置信度。

---

### 5.4 版面区块 (Block) — 语义段落单元
```json
{
  "id": "p1_b0001",
  "type": "heading",
  "text": "委托制作采购订单",
  "bbox": [210.50, 45.20, 385.00, 72.80],
  "item_ids": ["p1_i0001"]
}
```
- `type` [String, **REQUIRED**]：枚举值限定为：
  `heading` (标题), `paragraph` (正文段落), `list` (列表项), `table` (表格占据区), `figure` (图表说明), `header` (页眉), `footer` (页脚), `footnote` (脚注), `other`。
- `item_ids` [Array<String>, **REQUIRED**]：构成此区块的 TextItem ID 列表，**MUST 按阅读语序排列**。

---

### 5.5 表格 (Table & TableCell) — 结构化单元
```json
{
  "id": "p1_t0001",
  "bbox": [50.00, 300.00, 545.00, 500.00],
  "rows": 3,
  "cols": 4,
  "cells": [
    {
      "row_index": 0,
      "col_index": 0,
      "row_span": 1,
      "col_span": 1,
      "text": "商品名称",
      "bbox": [50.00, 300.00, 150.00, 330.00],
      "item_ids": ["p1_i0020"]
    }
  ]
}
```
- 支持任意单元格跨行跨列（`row_span`, `col_span` 默认为 1）。
- 每个单元格拥有独立 bbox 及对应内部关联的 `item_ids`。

---

## 6. 引用图完整性约束 (Referential Integrity Constraints)

合规的 Document IR 验证器 **MUST** 强制执行以下完整性校验：

1. **无悬空引用 (No Dangling References)**：
   - 任何 `Block.item_ids` 和 `TableCell.item_ids` 中的 ID，**MUST 能够在当前页面的 `items` 列表中寻址找到**。
   - 引用了不存在的 `item_id` 将被判定为违反规范（`CoreError::Validation("Dangling item_id reference")`）。
2. **唯一性不变量 (ID Uniqueness)**：
   - 文档内所有 `items[].id`、`blocks[].id`、`tables[].id` **MUST 保证全局唯一**，禁止重复。
3. **空间包含合理性 (Spatial Containment)**：
   - `Block.bbox` **SHOULD** 在几何上包络其所包含的所有 `items` 的外接矩形（允许 1 pt 内微小浮点误差）。

---

## 7. 第三方系统接入实现指南

### 7.1 第三方解析器开发者指南（实现 Layer 1）
如果你想用自建引擎（如基于 Python 的 PaddleOCR、Docling 或 AWS Textract）替代 LiteParse：
1. 运行你的 OCR / PDF 解析流水线。
2. 将字符/行坐标统一换算为左上角原点点坐标 `[x0, y0, x1, y1]`。
3. 顺序为图元编号，分配 `p{page}_i{:04}`。
4. 序列化为符合 `document-ir.v1.json` 的 JSON 文件。
5. **直接调用 Duon 抽取服务**：
   ```http
   POST /v1/extract HTTP/1.1
   Content-Type: application/json

   {
     "document_ir": { ...你的 IR 输出... },
     "schema": {
       "type": "object",
       "properties": {
         "contract_no": { "type": "string" },
         "total_price": { "type": "number" }
       }
     }
   }
   ```
   Duon 将立即基于你的 IR 输出高质量结构化数据与坐标证据，**无需安装任何 Duon 解析组件**。

---

### 7.2 第三方语义提取开发者指南（消费 Layer 1 输出）
如果你只想使用 Duon 高性能的 LiteParse 容器解析能力，但要在自己系统内写 Prompt 提取：
1. 调用 Duon 解析接口：
   ```http
   POST /v1/parse HTTP/1.1
   Content-Type: application/json

   {
     "document": { "url": "https://example.com/file.pdf" }
   }
   ```
2. 接收返回的标准 `Document IR`。
3. 直接使用其提供的 `p{page}_i0001` ID 格式拼接给你的大模型 Prompt。
4. 用户前端直接根据 `bbox: [x0, y0, x1, y1]` 和 `width/height` 在 PDF 预览器上画高亮遮罩，格式标准统一。

---

## 8. 标准 Schema 机器验证文件
本规范与以下机器可读的 JSON Schema 100% 互锁：
- **JSON Schema (Draft 2020-12)**：[`specs/json-schema/document-ir.v1.json`](file:///Users/aduang/github/duon/specs/json-schema/document-ir.v1.json)
