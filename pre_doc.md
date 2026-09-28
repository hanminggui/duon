对，你这次补充的信息会让我把方案进一步收敛。

**结论先给出来：**

> **LiteParse 负责全部“文档解析”工作；你的系统自己负责“文档语义理解、Schema 抽取、证据绑定、校验、结果规范化”。**
>
> 你的系统应该做成一个独立的 **Document Intelligence Service**，对外暴露统一 HTTP API；内部采用“LiteParse + 可替换本地 LLM + 确定性规则/校验”的架构。
>
> 第一版我建议：**Rust + Axum + LiteParse + PostgreSQL + MinIO/S3 + Qwen3-8B/14B + vLLM/llama.cpp**。

LiteParse 当前已经覆盖 PDF 文本解析、OCR、bbox、layout blocks、table、图片、Office 文档转换，并提供 Rust/Node/TypeScript/Python/WASM 绑定，所以没有必要再搭第二套 PDF/OCR 解析框架。它的 OCR 默认可以走 Tesseract，也支持通过 LiteParse 的 OCR HTTP 接口接入 PaddleOCR 等引擎；2026-09-09 的 LiteParse v2.14.4 官方 benchmark 中，LiteParse + PaddleOCR 在其自测 corpus 上的 reading-order/table 等指标也明显优于它同时测试的 pdf-inspector 1.19.0。([GitHub][1])

---

# 一、先把 Document IR 这件事情说清楚

你刚才问：

> Document IR 的作用是什么？目的是？

这个问题很关键。

## 它不是为了“再解析一次 PDF”

也不是为了跟 LiteParse 的 JSON 重复造轮子。

它的真正目的只有一个：

> **建立你自己的、稳定的“文档事实中间层”，把 LiteParse 和上面的 LLM/业务 API 解耦。**

可以理解成：

```text
                    LiteParse
                        │
                        ▼
              ┌──────────────────┐
              │   Document IR     │
              │                  │
              │ pages            │
              │ text_items       │
              │ blocks           │
              │ tables           │
              │ images           │
              │ bbox             │
              └────────┬─────────┘
                       │
             ┌─────────┴─────────┐
             ▼                   ▼
       Schema Extraction    Generic Extraction
             │                   │
             └─────────┬─────────┘
                       ▼
                     JSON
```

---

# 二、为什么不能直接把 LiteParse JSON 给 LLM？

其实**可以**。

你的第一版甚至完全可以：

```text
LiteParse JSON
      ↓
Qwen
      ↓
JSON
```

但是一旦你的系统成为独立服务，就会出现几个问题。

### 1. LiteParse 升级怎么办？

今天：

```text
LiteParse 2.14.4
```

以后：

```text
LiteParse 2.15
```

字段可能变化。

如果你的业务代码直接依赖：

```text
LiteParse.pages[].text_items[]
LiteParse.pages[].blocks[]
```

整个业务层都会跟着变。

而有了 IR：

```text
LiteParse 2.x
       ↓
DocumentIR v1
       ↓
业务层永远只认 DocumentIR v1
```

升级 parser 只是改 adapter。

---

### 2. 将来不只有 PDF

你现在说：

> PDF 等文件

而 LiteParse 当前本身已经支持将 Word、PowerPoint、Excel 等格式转换后再解析，同时也支持图片。([GitHub][1])

最终可能变成：

```text
PDF
DOCX
XLSX
PPTX
PNG
JPG
...
```

对于上层来说全部统一：

```text
DocumentIR
```

---

### 3. Evidence 必须有稳定 ID

比如：

```text
p1_i001
p1_i002
p1_i003
```

然后 LLM 返回：

```json
{
  "amount": 1600,
  "evidence_ids": ["p1_i006"]
}
```

你的程序自己查：

```text
p1_i006
→ page 1
→ bbox
→ 原文
```

这样**模型不负责坐标计算**。

---

### 4. IR 可以成为整个系统的“事实底座”

后面：

```text
Extract
Verify
Search
Highlight
Audit
Reprocess
Evaluate
```

都围绕这个 IR。

所以我建议：

> **Document IR 很薄，不做复杂语义，只负责规范化 LiteParse 输出和建立稳定引用。**

这是最重要的一点。

---

# 三、你的 Document IR 不应该很复杂

我建议就是：

```json
{
  "document_id": "doc_xxx",
  "source": {
    "sha256": "...",
    "mime_type": "application/pdf",
    "filename": "order.pdf"
  },

  "parser": {
    "name": "liteparse",
    "version": "2.14.4",
    "config_hash": "..."
  },

  "pages": [
    {
      "page": 1,
      "width": 595.32,
      "height": 841.92,

      "items": [
        {
          "id": "p1_i001",
          "type": "text",
          "text": "订单编号：2026-G-D-030",
          "bbox": [388.63, 142.85, 521.90, 154.49]
        }
      ],

      "blocks": [],
      "tables": []
    }
  ]
}
```

LiteParse 本身已经能输出 `text_items`、bbox、layout blocks，而且 blocks 可以区分 heading、paragraph、list、table、figure 等；table cell 也有自己的 bbox。([GitHub][1])

所以你的 IR **不要重复实现这些能力**。

---

# 四、你的系统真正应该增加的是“语义层”

我建议整个系统分成四个明确层次：

```text
L0  Document Parsing
        │
        │ LiteParse
        ▼
L1  Document IR
        │
        │ Context / Candidate Retrieval
        ▼
L2  Semantic Extraction
        │
        │ Qwen / other LLM
        ▼
L3  Validation & Normalization
        │
        ▼
     Final JSON
```

然后额外建立：

```text
Evidence
Audit
Correction
```

---

# 五、对外 API 不要暴露 LiteParse

这是我认为你现在设计上非常重要的一点。

调用方不应该知道：

```text
LiteParse
PDFium
Tesseract
PaddleOCR
Qwen
vLLM
```

调用方只知道：

```text
POST /v1/extract
```

例如：

```json
{
  "document": {
    "url": "https://example.com/order.pdf"
  },

  "schema": {
    "type": "object",
    "properties": {
      "party_a": {
        "type": "string",
        "description": "甲方/委托方名称"
      },
      "party_b": {
        "type": "string",
        "description": "乙方/供应商名称"
      },
      "amount": {
        "type": "number",
        "description": "合同或订单总金额"
      },
      "order_date": {
        "type": "string",
        "description": "下单时间"
      }
    },
    "required": [
      "party_a",
      "party_b",
      "amount",
      "order_date"
    ]
  }
}
```

结果：

```json
{
  "data": {
    "party_a": "吾立方公司",
    "party_b": "宋泽昊",
    "amount": 1600,
    "order_date": "2026-05-18"
  },

  "evidence": {
    "/party_a": [
      {
        "page": 1,
        "item_ids": ["p1_i004"],
        "quote": "公司/部门 吾立方鸣潮 6_心月狐"
      }
    ],

    "/party_b": [
      {
        "page": 1,
        "item_ids": ["p1_i010"],
        "quote": "供应商：宋泽昊"
      }
    ],

    "/amount": [
      {
        "page": 1,
        "item_ids": ["p1_i006"],
        "quote": "费用价格：总金额为人民币 1600 元 税前。"
      }
    ]
  },

  "meta": {
    "document_sha256": "...",
    "model": "Qwen3-8B",
    "model_version": "...",
    "pipeline_version": "1.0.0"
  }
}
```

这个设计有一个非常大的好处：

> **调用者自己的 Schema 完全不被你的 Evidence/Confidence/Meta 污染。**

`data` 严格符合调用方 Schema。

Evidence 独立存在。

---

# 六、为什么 Evidence 应该独立于 data？

因为用户可能定义：

```json
{
  "amount": {
    "type": "number"
  }
}
```

你却希望返回：

```json
{
  "amount": 1600,
  "evidence": ...
}
```

那就破坏了用户的 Schema。

所以应该：

```text
response
├── data       ← 严格遵守调用者 Schema
├── evidence   ← 你的系统附加信息
├── validation
└── meta
```

非常干净。

---

# 七、你需要同时支持两种工作模式

这是你的系统与普通“PDF 转 JSON”服务最大的区别。

## Mode 1：Schema Extraction

调用方明确告诉你：

> 我要这些字段。

例如：

```json
{
  "schema": {
    "party_a": "...",
    "party_b": "...",
    "amount": "...",
    "delivery_date": "..."
  }
}
```

这是**高准确率模式**。

系统可以只围绕这些字段工作。

---

# 八、Mode 2：Generic Extraction

调用方什么都不告诉你：

```json
{
  "document": {
    "url": "..."
  }
}
```

系统也必须返回有意义的 JSON。

但这里有一个很重要的产品设计原则：

> **不要要求模型凭空生成一个完全自由的 JSON Schema。**

这样会导致：

```text
合同 A：

{
  "party_a": ...
}

合同 B：

{
  "甲方": ...
}

合同 C：

{
  "client": ...
}
```

根本无法使用。

---

# 九、Generic Mode 应该有一个固定的“通用文档 Schema”

例如：

```json
{
  "document_type": "purchase_order",

  "title": "...",

  "facts": [
    {
      "key": "订单编号",
      "value": "2026-G-D-030"
    },
    {
      "key": "供应商",
      "value": "宋泽昊"
    }
  ],

  "entities": [
    {
      "text": "张思怡",
      "type": "person"
    },
    {
      "text": "吾立方公司",
      "type": "organization"
    }
  ],

  "dates": [
    {
      "text": "2026 年 5 月 18 日",
      "type": "order_date",
      "normalized": "2026-05-18"
    }
  ],

  "amounts": [
    {
      "text": "1600 元",
      "value": 1600,
      "currency": "CNY"
    }
  ],

  "sections": []
}
```

这样未知文档也能工作。

例如用户给你：

```text
发票
合同
报价单
采购单
工资单
简历
报告
会议纪要
说明书
产品规格
```

都至少能返回：

```text
document_type
title
facts
entities
dates
amounts
sections
tables
```

未知类型：

```json
{
  "document_type": "other"
}
```

不会因为模型“不知道这个是什么”而整个请求失败。

---

# 十、所以 Generic Mode 和 Schema Mode 实际上共享 80% 的底层能力

```text
                  LiteParse
                      │
                 Document IR
                      │
               Context Builder
                      │
             ┌────────┴─────────┐
             │                  │
       Schema Provided      No Schema
             │                  │
       Targeted Extract    Generic Extract
             │                  │
             └────────┬─────────┘
                      ▼
                 Normalize
                      ▼
                 Validate
                      ▼
                 Evidence
```

这是非常合理的架构。

---

# 十一、我建议再加一个第三个 API：Verify

因为你最初真正的业务需求其实是：

> PDF 对系统已有结构化数据进行核验。

所以独立系统以后应该有：

```text
POST /v1/extract
POST /v1/verify
POST /v1/parse
```

---

## `/v1/parse`

只做：

```text
URL
 ↓
LiteParse
 ↓
Document IR
```

给高级调用方。

---

## `/v1/extract`

做：

```text
URL
+
Schema
 ↓
JSON
+
Evidence
```

---

## `/v1/verify`

做：

```json
{
  "document": {
    "url": "..."
  },

  "schema": {...},

  "data": {
    "party_a": "吾立方公司",
    "party_b": "宋泽昊",
    "amount": 1500,
    "delivery_date": "2026-05-19"
  }
}
```

返回：

```json
{
  "status": "conflict",

  "fields": {
    "amount": {
      "system_value": 1500,
      "document_value": 1600,
      "status": "conflict",

      "evidence": [
        {
          "page": 1,
          "quote": "总金额为人民币 1600 元 税前。"
        }
      ]
    }
  }
}
```

这一下就与你最开始的业务直接打通了。

---

# 十二、而且 Verify 模式可以比 Extract 快很多

因为：

```text
Extract
```

是：

> “帮我找出整个文档是什么。”

而：

```text
Verify
```

是：

> “我只关心这 8 个字段。”

例如数据库已经有：

```text
party_a
party_b
amount
delivery_date
supplier_phone
```

系统只搜索这些字段附近的证据。

于是：

```text
输入文档
 ↓
LiteParse
 ↓
定位相关 blocks
 ↓
Qwen
 ↓
验证 5 个字段
```

甚至不需要让模型读完整 100 页合同。

---

# 十三、这也决定了你的 LLM 架构

我不建议：

```text
PDF → Agent → Agent → Agent → JSON
```

这个问题**不是 Agent 问题**。

应该是一个确定性的 Pipeline：

```text
Fetch
 ↓
Parse
 ↓
Normalize
 ↓
Retrieve
 ↓
Extract
 ↓
Validate
 ↓
Canonicalize
 ↓
Respond
```

不要上 LangChain/CrewAI/GPT Agent 一类复杂 orchestration。

你的系统最重要的是：

> **可重复、可验证、可测试。**

不是“自主行动”。

---

# 十四、LLM 选型

现在我会这样做：

| Profile         | 模型                  | 用途                |
| --------------- | ------------------- | ----------------- |
| fast            | **Qwen3-8B**        | 默认生产模型            |
| balanced        | **Qwen3-14B**       | 高准确率              |
| high            | 更大的 Qwen / 当前高端本地模型 | 复杂文档              |
| vision fallback | 暂不需要                | LiteParse 已负责 OCR |

Qwen3-8B 是 8.2B 参数，Qwen3-14B 是 14.8B 参数，两者都支持 32K 原生上下文、YaRN 扩展到 131K，并且官方直接提供 vLLM/SGLang 等部署方式。([Hugging Face][2])

而且 Qwen3 本身支持 thinking / non-thinking 两种模式。([Hugging Face][2])

对你的生产抽取任务：

```text
enable_thinking = false
temperature = 0
```

作为默认。

只有特别复杂的冲突判断，才考虑第二次 reasoning。

---

# 十五、当前还有 Qwen3.8，但我不会第一版就上

2026 年已经有 Qwen3.8 系列，例如 Qwen3.8-27B。([Hugging Face][3])

但对于你的目标：

> 自部署 + 没有大规模 GPU + 文档抽取

我不会从 27B 起步。

正确做法是把 LLM 做成：

```text
ModelBackend
```

例如：

```yaml
models:
  fast:
    model: Qwen3-8B

  balanced:
    model: Qwen3-14B

  high:
    model: Qwen3.8-27B
```

将来换模型不改系统。

---

# 十六、推理服务：我首选 vLLM

你的 LLM service 不要直接嵌进业务服务。

独立：

```text
Document Service
       │
       │ OpenAI-compatible API
       ▼
    vLLM
       │
       ▼
   Qwen3-8B/14B
```

vLLM 当前原生支持 structured outputs / JSON Schema。([vLLM][4])

而且 vLLM 已经有专门的 reproducibility 和 batch invariance 能力。([vLLM][5])

---

# 十七、CPU 模式则用 llama.cpp

你没有 GPU 的机器也应该能部署：

```text
Document Service
       │
       ▼
 llama.cpp
       │
       ▼
 Qwen3-8B GGUF
```

Qwen 官方已经提供 Qwen3-8B GGUF；llama.cpp server 支持 CPU/GPU、OpenAI-compatible API、continuous batching、schema-constrained JSON。([Hugging Face][6])

不过我会把：

```text
vLLM
```

作为主力。

```text
llama.cpp
```

作为：

* CPU 部署
* 边缘部署
* 开发环境
* 极低成本部署

---

# 十八、为什么不是 Ollama？

Ollama 可以跑。

但你现在做的是：

> **一个供其他系统调用的后端服务。**

不是：

> “开发机器上跑一个模型聊天”。

所以：

```text
vLLM
>
llama.cpp
>
Ollama
```

是我针对这个项目的优先级。

Ollama 更适合：

```text
开发
测试
单机
低并发
```

---

# 十九、你要求“同一份文档多次处理结果一致”，这个必须专门设计

这个要求非常重要。

而且：

> **缓存 ≠ 确定性。**

你明确说：

> 不是靠缓存。

完全正确。

---

# 二十、真正需要保证的是：

对于：

```text
相同 Document Bytes
+
相同 Parser Version
+
相同 Parser Config
+
相同 Schema
+
相同 Prompt Version
+
相同 Model Version
+
相同 Inference Config
```

得到：

```text
相同 Semantic Result
```

---

# 二十一、第一步：不是 hash URL，而是 hash 文件内容

例如：

```text
https://example.com/a.pdf
```

不能作为文档身份。

因为：

```text
今天 a.pdf = 版本 A
明天 a.pdf = 版本 B
```

应该：

```text
download
 ↓
SHA256(file_bytes)
 ↓
document_sha256
```

所以：

```json
{
  "source_sha256": "abc123..."
}
```

才是文档的内容身份。

---

# 二十二、第二步：固定整个 Pipeline Version

例如：

```json
{
  "pipeline": {
    "version": "1.0.3",

    "parser": "liteparse@2.14.4",

    "ocr": "tesseract@...",
    
    "ir_schema": "1.0",

    "prompt": "contract-extract@17",

    "model": "Qwen3-8B@sha256:xxxx",

    "inference": {
      "temperature": 0,
      "seed": 0
    }
  }
}
```

这样你的结果真正具备可复现性。

---

# 二十三、LLM 的“确定性”需要特别小心

不能简单说：

```text
temperature=0
```

就认为：

> 100% 永远一样。

vLLM 当前文档明确说明，默认情况下不保证跨运行完全 reproducible；其 reproducibility 能力还受硬件和 vLLM 版本影响，并提供 batch invariance 来让输出不受 batch size/order 影响。([vLLM][5])

所以你的生产策略应该是：

```text
temperature = 0
seed = 固定
```

同时：

```text
固定模型文件
固定 vLLM 版本
固定 CUDA/driver
固定硬件 profile
```

如果你要求：

> 并发从 1 个变成 32 个，请求结果也必须完全一致

那么进一步启用：

```text
batch invariance
```

并在你的固定部署版本上做回归测试。vLLM 当前已经提供这个能力，但官方仍标注为 beta，所以要把它纳入自己的 CI 验证，而不是盲信。([vLLM][7])

---

# 二十四、还有一个经常被忽略的问题：数组顺序

假如模型返回：

```json
{
  "contacts": [
    {"name": "张思怡"},
    {"name": "宋泽昊"}
  ]
}
```

下一次：

```json
{
  "contacts": [
    {"name": "宋泽昊"},
    {"name": "张思怡"}
  ]
}
```

语义一样，但 JSON 不一样。

所以必须定义：

```text
array ordering policy
```

例如：

```text
先 page
再 y
再 x
再 type
```

然后程序自己排序。

**不要让 LLM 决定最终数组顺序。**

---

# 二十五、最终还要做 Canonical JSON

最终响应做：

```text
normalize
 ↓
sort keys
 ↓
normalize dates
 ↓
normalize numbers
 ↓
sort unordered arrays
 ↓
canonical JSON
```

这样：

```text
Same Input
      ↓
Same semantic result
      ↓
Same canonical JSON
      ↓
Same SHA256
```

你甚至可以给结果一个：

```text
result_hash
```

这样外部调用者可以验证。

---

# 二十六、整个系统我建议这样部署

```text
                         Internet / Internal Network
                                  │
                                  ▼
                         ┌─────────────────┐
                         │ API / Gateway   │
                         └────────┬────────┘
                                  │
                                  ▼
                    ┌────────────────────────┐
                    │ Document Intelligence  │
                    │ Service                │
                    │                        │
                    │ Rust + Axum + Tokio    │
                    │                        │
                    │ URL Fetcher             │
                    │ Document IR             │
                    │ Context Builder         │
                    │ Extraction              │
                    │ Validation              │
                    │ Canonicalization        │
                    └───────┬────────┬───────┘
                            │        │
                ┌───────────┘        └───────────┐
                ▼                                ▼
        ┌─────────────────┐              ┌──────────────┐
        │ LiteParse Worker│              │ LLM Server   │
        │ Rust             │              │              │
        │                  │              │ vLLM         │
        │ PDF/OCR/layout   │              │ Qwen3        │
        └────────┬─────────┘              └──────────────┘
                 │
                 ▼
        ┌─────────────────┐
        │ PostgreSQL      │
        │ jobs/results    │
        │ schemas         │
        │ versions        │
        └─────────────────┘
                 │
                 ▼
        ┌─────────────────┐
        │ MinIO / S3      │
        │ source PDF      │
        │ screenshots     │
        │ artifacts       │
        └─────────────────┘
```

---

# 二十七、技术选型我会这样定

| 层                 | 推荐                                       |
| ----------------- | ---------------------------------------- |
| API               | **Rust + Axum**                          |
| Async Runtime     | **Tokio**                                |
| PDF/Office/图片解析   | **LiteParse**                            |
| OCR               | **LiteParse 内置 OCR / LiteParse OCR API** |
| Document IR       | 自定义轻量 Rust struct                        |
| JSON Schema       | JSON Schema                              |
| Schema Validation | `jsonschema` + `serde`                   |
| LLM               | **Qwen3-8B / Qwen3-14B**                 |
| GPU Inference     | **vLLM**                                 |
| CPU Inference     | **llama.cpp**                            |
| DB                | **PostgreSQL**                           |
| Object Storage    | **MinIO / S3**                           |
| Queue 第一阶段        | **PostgreSQL Job Queue**                 |
| 高并发再升级            | **NATS JetStream**                       |
| Monitoring        | Prometheus + Grafana                     |
| Tracing           | OpenTelemetry                            |
| API 文档            | OpenAPI                                  |
| Container         | Docker                                   |
| 部署                | Docker Compose → K8s                     |
| Embedding         | **第一版不要**                                |
| Vector DB         | **第一版不要**                                |
| Agent Framework   | **不要**                                   |

---

# 二十八、为什么 Queue 第一版甚至不需要 Redis

你的业务是：

```text
上传/下载文件
→ PDF 解析
→ LLM
```

天然是：

```text
job
```

没必要一开始：

```text
Postgres
Redis
RabbitMQ
Kafka
NATS
```

全装。

第一版：

```text
PostgreSQL jobs
```

用：

```sql
SELECT ...
FOR UPDATE SKIP LOCKED
```

就够了。

以后真的到：

```text
数百/数千 job/s
```

再切：

```text
NATS JetStream
```

---

# 二十九、但 URL Fetcher 一定要单独设计

因为你的 API 是：

```json
{
  "url": "https://xxx.com/a.pdf"
}
```

这是一个典型的 SSRF 攻击面。

必须：

```text
只允许 http/https
        ↓
DNS 解析
        ↓
禁止：
127.0.0.1
10.0.0.0/8
172.16/12
192.168/16
169.254.169.254
IPv6 link-local
        ↓
请求
        ↓
重定向重新检查
```

另外：

```text
最大文件大小
最大页数
最大下载时间
最大解析时间
最大 OCR 时间
最大内存
```

全部要限制。

否则一个恶意 URL：

```text
http://internal-service/
```

或者一个：

```text
20GB PDF
```

就可以把整个服务打死。

---

# 三十、PDF Parser Worker 应该沙箱化

你面对的是：

> 外部用户上传的任意 PDF。

所以不要直接：

```text
HTTP API process
    ↓
LiteParse
```

我更建议：

```text
API
 ↓
Job
 ↓
Parse Worker Container
 ↓
LiteParse
```

这个 container：

```text
无网络
只读文件系统
CPU limit
memory limit
PID limit
timeout
```

解析失败直接杀掉 worker。

LiteParse 官方在 Python/Node 侧已经提供持久 worker、并行处理和硬超时机制；它的底层使用 PDFium，因此生产环境仍建议把文档解析视为不可信输入处理。([GitHub][1])

---

# 三十一、这里我反而建议你不要把“LiteParse 4 种 SDK”当成系统架构选择

你说 LiteParse 有：

```text
TypeScript
Python
Rust
Browser
```

这很好。

但是你的系统对外并不需要暴露：

```text
npm liteparse
pip liteparse
cargo liteparse
```

而是：

```text
你的 HTTP API
```

所以调用方：

```text
Go
Java
Python
Node
PHP
C#
Rust
```

全部：

```http
POST /v1/extract
```

即可。

你甚至可以：

```text
OpenAPI
 ↓
自动生成
TS SDK
Python SDK
Go SDK
Rust SDK
Java SDK
```

这比让调用者直接依赖 LiteParse 强很多。

---

# 三十二、Document IR 其实还会给你一个非常大的好处：以后可以不重复 OCR

例如：

```text
第一次
PDF
 ↓
LiteParse
 ↓
DocumentIR
```

然后：

```text
Schema A
 ↓
extract

Schema B
 ↓
extract

Schema C
 ↓
extract

Verify
 ↓
verify
```

全部复用同一个：

```text
DocumentIR
```

注意：

这不是你说的“通过缓存让结果一致”。

这是：

> **DocumentIR 是一次确定性的中间产物，而不是 LLM 结果缓存。**

同一份文档要换 10 个 Schema，没有必要重新 OCR 10 次。

---

# 三十三、整个系统最重要的版本对象应该有 5 个

```text
Document
Parser
IR
Schema
Model
```

最终结果：

```text
Result =
    Hash(
        document_sha256
        + parser_version
        + parser_config_hash
        + ir_version
        + schema_hash
        + prompt_version
        + model_version
        + inference_config
    )
```

这会让你的系统天然适合：

```text
审计
回放
A/B Test
模型升级
Regression Test
```

---

# 三十四、最终建议的目录结构

如果现在让我直接开项目，我会这样：

```text
document-intelligence/
│
├── crates/
│   ├── core/
│   │   ├── document_ir/
│   │   ├── schema/
│   │   ├── evidence/
│   │   ├── normalization/
│   │   └── validation/
│   │
│   ├── parser/
│   │   └── liteparse_adapter/
│   │
│   ├── extractor/
│   │   ├── llm/
│   │   ├── context/
│   │   ├── generic/
│   │   └── schema_extract/
│   │
│   └── api/
│       ├── extract/
│       ├── verify/
│       ├── parse/
│       └── jobs/
│
├── migrations/
├── prompts/
├── schemas/
├── tests/
│   ├── golden/
│   ├── reproducibility/
│   ├── parser/
│   └── extraction/
│
├── docker/
│   ├── api/
│   ├── parser/
│   └── model/
│
└── deploy/
    ├── docker-compose.yml
    └── k8s/
```

---

# 三十五、第一阶段千万不要做的东西

为了控制系统复杂度，我会明确砍掉：

```text
❌ LangChain
❌ CrewAI
❌ Agent Loop
❌ Vector DB
❌ Elasticsearch
❌ RAG
❌ Fine-tune
❌ 多个 OCR 框架
❌ 多个 PDF Parser
❌ Embedding
❌ Workflow Engine
```

第一阶段只需要：

```text
LiteParse
+
DocumentIR
+
Qwen
+
Rule
+
Evidence
+
Postgres
```

---

# 三十六、第一阶段真正应该验证的不是模型 benchmark

你应该做一套：

## Document Extraction Benchmark

例如 100～500 个真实文件。

字段：

```text
甲方
乙方
联系人
手机号
金额
币种
含税/未税
合同编号
签约时间
下单时间
交付时间
付款条件
项目名称
产品名称
数量
单价
总价
```

分别测：

```text
Schema Extraction Accuracy
Evidence Accuracy
Hallucination Rate
Missing Rate
Conflict Detection Recall
Exact JSON Reproducibility
P50 Latency
P95 Latency
Memory
CPU
GPU
```

---

# 三十七、我现在给你的最终技术路线

## 生产架构

```text
                 ┌──────────────────┐
                 │      Client      │
                 └────────┬─────────┘
                          │
                          ▼
                 ┌──────────────────┐
                 │ Rust + Axum API  │
                 └────────┬─────────┘
                          │
              ┌───────────┼────────────┐
              ▼           ▼            ▼
           /parse      /extract      /verify
              │           │            │
              └───────────┼────────────┘
                          ▼
                   URL Fetcher
                          │
                          ▼
                    LiteParse
                          │
              ┌───────────┴───────────┐
              ▼                       ▼
         text_items                 blocks
              │                       │
              └───────────┬───────────┘
                          ▼
                    Document IR
                          │
                          ▼
                  Context Builder
                          │
                 ┌────────┴────────┐
                 ▼                 ▼
           Schema Mode        Generic Mode
                 │                 │
                 └────────┬────────┘
                          ▼
                    Qwen3-8B
                          │
                  low confidence?
                       │
                 ┌─────┴─────┐
                 ▼           ▼
              accept      Qwen3-14B
                              │
                              ▼
                       Rule Validation
                              │
                              ▼
                       Evidence Binding
                              │
                              ▼
                       Canonical JSON
                              │
                              ▼
                         PostgreSQL
```

---

# 三十八、最后给你一个非常明确的选型结论

### Parser

**直接定 LiteParse。**

你的实际输出已经证明它满足你当前需求，而且当前版本已经有：

* OCR
* native text extraction
* bbox
* blocks
* tables
* image
* Office conversion
* Rust/TS/Python/WASM
* OCR server 扩展
* complexity detection

所以没有必要再引入一个 PaddleOCR-VL 作为第二套“解析主线”。PaddleOCR 可以作为 LiteParse 的 OCR backend，而不是你的第二个 parser。([GitHub][1])

### Document IR

**保留，但做成很薄的 Canonical Document Model。**

它的目的不是“增强解析”，而是：

> **解耦 LiteParse、建立稳定证据 ID、统一不同文档类型、支撑多次 Schema 抽取和审计。**

### LLM

第一版：

```text
Qwen3-8B
```

默认：

```text
non-thinking
temperature=0
structured output
```

高准确率 profile：

```text
Qwen3-14B
```

两者当前都有官方 vLLM/SGLang/本地部署支持。([Hugging Face][2])

### Inference

```text
GPU → vLLM
CPU/低资源 → llama.cpp
```

vLLM 对 structured output 和 reproducibility 的支持更适合作为服务端主力；llama.cpp 则很适合低资源部署。([vLLM][4])

### Backend

```text
Rust + Axum
PostgreSQL
MinIO/S3
```

### Queue

第一版：

```text
PostgreSQL jobs
```

以后再：

```text
NATS JetStream
```

### API

核心三个：

```text
POST /v1/parse
POST /v1/extract
POST /v1/verify
```

### 最大的设计原则

```text
LiteParse = 看懂文档
DocumentIR = 保存文档事实
LLM = 理解业务语义
Schema = 定义我要什么
Evidence = 证明为什么这么说
Validator = 判断是否合法
Canonicalizer = 保证结果稳定
```

这套分工非常适合你现在的目标。

而且你最初的“**PDF 检查系统结构化数据并修正**”，实际上只是这个系统中的 `/verify` 能力，并不需要单独再造一套系统。

[1]: https://github.com/run-llama/liteparse "GitHub - run-llama/liteparse: A fast, helpful, and open-source document parser · GitHub"
[2]: https://huggingface.co/Qwen/Qwen3-14B "Qwen/Qwen3-14B · Hugging Face"
[3]: https://huggingface.co/Qwen/Qwen3.8-27B?utm_source=chatgpt.com "Qwen/Qwen3.8-27B"
[4]: https://docs.vllm.ai/en/stable/examples/features/structured_outputs/?utm_source=chatgpt.com "Structured Outputs - vLLM"
[5]: https://docs.vllm.ai/en/latest/usage/reproducibility/ "Reproducibility - vLLM"
[6]: https://huggingface.co/Qwen/Qwen3-8B-GGUF/tree/main?utm_source=chatgpt.com "Qwen/Qwen3-8B-GGUF at main"
[7]: https://docs.vllm.ai/en/latest/features/batch_invariance/ "Batch Invariance - vLLM"

