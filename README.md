# Duon 智能文档理解服务

<p align="center">
  <strong>高精度、高确定性、证据可溯源的下一代文档智能中间件与服务引擎</strong>
</p>

<p align="center">
  <a href="#核心特性"><img src="https://img.shields.io/badge/Architecture-Single_Responsibility-blue.svg" alt="Architecture"></a>
  <a href="#规范与协议"><img src="https://img.shields.io/badge/Standard-Document_IR_v1.0-green.svg" alt="Document IR"></a>
  <a href="https://github.com/rust-lang/rust"><img src="https://img.shields.io/badge/Rust-1.80+-orange.svg" alt="Rust Version"></a>
  <a href="#安全防护"><img src="https://img.shields.io/badge/Security-SSRF_Protected_%26_Zero_Retention-red.svg" alt="Security"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg" alt="License"></a>
</p>

---

## 📖 简介 (Introduction)

**Duon** 是一个专为生产级企业应用构建的开源 **Document Intelligence Service**（智能文档理解与核验服务）。

传统的文档抽取方案往往直接将不可控的 PDF 文本丢入大模型（LLM），面临**坐标丢失、证据无法溯源、模型幻觉严重、重复 OCR 算力浪费**等致命缺陷。Duon 基于“**物理事实与业务语义彻底解耦**”与“**严格单一职责原则（SRP）**”，将文档智能清晰划分为两层独立且标准化的流水线：

1. **第 1 层（解析层）**：`PDF/文件 -> 文档事实解析 -> Document IR`（只看懂文档排版，输出带坐标与全局条目 ID 的客观事实，绝无大模型干预）。
2. **第 2 层（语义层）**：`Document IR + 约束 (Schema / 核验数据) -> LLM 推理 -> 规范化 JSON + 物理坐标证据`（只消费标准 IR，完全不碰 PDF 与 OCR，极速且确定）。

Duon 彻底摈弃不可控的 Agent 循环与臃肿框架，采用确定性流水线与 **Canonical JSON** 算法，实现**同输入同配置下字节级结果可复现（Deterministic SHA-256）**。

名字来自谐音梗, `doc -> duck`。本项目提供的核心服务是 文档转转`json` 支持类型安全

取`duck`的前半段`du` + `json`的后半段`on` = `duon`

---

## ✨ 核心特性 (Key Features)

- 🎯 **严格单一职责设计 (Strict SRP)**：解析（Parse）与抽取（Extract）物理隔离。无文档 URL 与 IR 的混杂逻辑，接口边界清晰透明。
- 📐 **开放标准 Document IR (v1.0)**：制定开放中立的文档事实中间表示协议。原点统一为左上角 `[x0, y0, x1, y1]`，自带拓扑防悬空引用校验，兼容第三方解析器（Docling, PaddleOCR 等）与第三方抽取器。
- 🔍 **零幻觉物理证据绑定 (Zero-Hallucination Evidence)**：模型仅返回条目引用 ID（如 `p1_i0004`），系统在底层自动反查几何坐标并合并外接矩形；任何伪造的无效 ID 会被 100% 自动剥离并拦截。
- 🔒 **企业级安全防御与零磁盘常驻**：
  - **URL Fetcher 防 SSRF & 内网策略**：内置 DNS 预检防 SSRF，默认拦截私有网段（RFC 1918）、环回与云厂商元数据（169.254.169.254）；支持通过 `DUON_ALLOW_PRIVATE_IPS` 或白名单可信放行企业内部私有存储（MinIO/S3）。
  - **EphemeralFile (RAII)**：临时上传文件通过 Rust 生命周期析构，离开作用域立即物理粉碎，保证磁盘零常驻。
- 📦 **全场景文件输入形态**：原生支持 HTTP/HTTPS URL、Form Data 表单文件上传、内联 Base64 / Data URI、原始二进制流直传（`application/pdf`）、本地挂载路径（PVC/NFS）。

- ⚡ **开箱即用的两层协作能力**：
  - **高吞吐低成本**：同一份 100 页大文档只需解析一次，更换 10 次 Schema 抽取只需调用第 2 层，节省 80% 以上算力。
  - **结构化业务核验 (Verify API)**：支持比对已有数据库记录与文档真实事实，输出精准一致性状态（`matched` / `conflict`）与差异原因说明。
- 🔄 **确定性输出规范 (Canonical JSON)**：内建 BTreeMap 字典序排序、Unicode NFC 规整与浮点规范，每次运行生成固化的 `result_hash`。
- 🌐 **广泛兼容 OpenAI 协议生态**：支持无缝对接 vLLM, llama.cpp, Ollama, DeepSeek, Qwen 等任何标准推理服务。

---

## 🏛 架构全景图 (Architecture)

```text
外部调用方 (HTTP Client / Frontend)
   │
   ├──────────────────────────────┬──────────────────────────────┐
   ▼                              ▼                              ▼
POST /v1/parse                POST /v1/extract               POST /v1/verify
[第 1 层：物理事实解析]        [第 2 层：语义结构化提取]      [第 2 层：结构化数据核验]
   │                              │                              │
   │ 输入: PDF 文件 (URL/Upload)   │ 强制输入: Document IR + Schema │ 强制输入: Document IR + Data
   ▼                              ▼                              ▼
┌──────────────────────────┐   ┌──────────────────────────┐   ┌──────────────────────────┐
│ crates/parser            │   │ crates/extractor         │   │ crates/extractor/verify  │
│ • LiteParse 适配器       │   │ • ContextBuilder 上下文  │   │ • 字段对齐与模糊容差比对 │
│ • 坐标系归一化 (左上角)  │   │ • OpenAI 协议适配器      │   │ • 冲突识别与 diff_reason │
│ • 稳定 ID 生成 (p1_i0001)│   │ • EvidenceBinder (防幻觉)│   │ • 证据反查与位置绑定     │
└────────────┬─────────────┘   └────────────┬─────────────┘   └────────────┬─────────────┘
             │                              │                              │
             ▼                              ▼                              ▼
       Document IR                   ExtractResponse                VerifyResponse
    (标准文档事实中间件)             (纯净 Data + Evidence)        (状态 + 差异证据)
```

---

## 🚀 快速开始 (Quick Start)

### 1. 编译环境要求
- **Rust**: 1.80.0 及以上版本（`cargo --version`）
- **OS**: Linux, macOS, Windows

### 2. 克隆与配置
```bash
git clone https://github.com/duon-ai/duon.git
cd duon

# 复制环境变量模版
cp .env.example .env
```

编辑 `.env` 配置你的大模型服务地址（本地 Ollama / vLLM 或外部 API 均可）：
```bash
# LLM 兼容接口地址 (例如本地 vLLM 或 Ollama)
LLM_BASE_URL=http://localhost:8000/v1

# 授权 API Key (无鉴权留空即可)
LLM_API_KEY=

# 默认调用的模型名称
LLM_MODEL=Qwen3-8B

# API 服务端口
PORT=8080
```

### 3. 运行单元与集成测试
Duon 遵循严格的 TDD（测试驱动开发）原则，全工作区已内建 15 项端到端及契约验证测试：
```bash
cargo test
```

### 4. 启动服务
```bash
cargo run -p duon-api
```
服务启动后将监听：`http://0.0.0.0:8080`。

---

## 📡 接口调用指南 (API Usage)

### 步骤 1：调用第 1 层解析文档，获取 Document IR
调用 `POST /v1/parse`，Duon 原生支持多种灵活的文件输入方式：

#### 方式 A：HTTP / HTTPS URL（支持公网与内网服务）
```bash
curl -X POST http://localhost:8080/v1/parse \
  -H "Content-Type: application/json" \
  -d '{
    "document": {
      "url": "http://10.0.1.20:9000/bucket/contract.pdf",
      "headers": {
        "Authorization": "Bearer internal-token"
      }
    },
    "ocr_enabled": true
  }'
```
> 💡 若文件服务来自企业内网（如私有 MinIO、内部 S3），只需在服务配置中开启 `DUON_ALLOW_PRIVATE_IPS=true` 或配置 `DUON_SSRF_ALLOWED_HOSTS` 白名单即可无缝接入。

#### 方式 B：Form Data (`multipart/form-data`) 表单直接上传
```bash
curl -X POST http://localhost:8080/v1/parse \
  -F "file=@/path/to/contract.pdf" \
  -F "ocr_enabled=true"
```
> 💡 上传文件在内存与临时目录流转，由 `EphemeralFile` RAII 守卫管理生命周期，解析完成即刻自动销毁，零常驻。

#### 方式 C：内联 Base64 或 Data URI
```bash
curl -X POST http://localhost:8080/v1/parse \
  -H "Content-Type: application/json" \
  -d '{
    "document": {
      "base64": "JVBERi0xLjQKJ..."
    }
  }'
```

#### 方式 D：原始二进制流直接 POST（极简高效）
```bash
curl -X POST "http://localhost:8080/v1/parse?ocr_enabled=true" \
  -H "Content-Type: application/pdf" \
  --data-binary @/path/to/contract.pdf
```

#### 方式 E：本地文件系统路径（私有化 / 共享卷挂载场景）
```bash
curl -X POST http://localhost:8080/v1/parse \
  -H "Content-Type: application/json" \
  -d '{
    "document": {
      "path": "/shared/storage/invoices/2026-09.pdf"
    }
  }'
```

**响应示例**（纯净客观物理事实，带全局唯一 ID 与绝对坐标）：

```json
{
  "document_id": "doc_e3b0c442...",
  "source": { "sha256": "...", "mime_type": "application/pdf" },
  "parser": { "name": "liteparse", "version": "2.14.4" },
  "pages": [
    {
      "page_number": 1,
      "width": 595.32,
      "height": 841.92,
      "items": [
        {
          "id": "p1_i0001",
          "type": "text",
          "text": "委托制作采购订单",
          "bbox": [210.50, 45.20, 385.00, 72.80]
        },
        {
          "id": "p1_i0004",
          "type": "text",
          "text": "公司/部门：吾立方公司",
          "bbox": [50.00, 180.00, 185.00, 192.00]
        }
      ],
      "blocks": []
    }
  ]
}
```

---

### 步骤 2：调用第 2 层进行语义抽取（纯 IR 输入，零 OCR 开销）
把上一步获得的 `document_ir` 直接传入 `POST /v1/extract`，附带目标业务 JSON Schema：

```bash
curl -X POST http://localhost:8080/v1/extract \
  -H "Content-Type: application/json" \
  -d '{
    "document_ir": { ...步骤1获得的 Document IR... },
    "schema": {
      "type": "object",
      "required": ["party_a", "amount"],
      "properties": {
        "party_a": { "type": "string", "description": "委托公司" },
        "amount": { "type": "number", "description": "总金额" }
      }
    }
  }'
```

**响应示例**（业务数据与物理证据解耦，附带确定性哈希）：
```json
{
  "data": {
    "party_a": "吾立方公司",
    "amount": 1600
  },
  "evidence": {
    "/party_a": [
      {
        "page": 1,
        "item_ids": ["p1_i0004"],
        "quote": "公司/部门：吾立方公司",
        "bbox": [50.00, 180.00, 185.00, 192.00],
        "confidence": 0.99
      }
    ]
  },
  "validation": {
    "is_valid": true,
    "errors": []
  },
  "meta": {
    "document_sha256": "e3b0c442...",
    "pipeline_version": "1.0.0",
    "result_hash": "7f83b165...",
    "model": "Qwen3-8B",
    "execution_time_ms": 780
  }
}
```

---

### 步骤 3：调用第 2 层进行业务数据核验 (Verify)
将待验证的业务数据库记录与 `document_ir` 传入 `POST /v1/verify`：

```bash
curl -X POST http://localhost:8080/v1/verify \
  -H "Content-Type: application/json" \
  -d '{
    "document_ir": { ...步骤1获得的 Document IR... },
    "data": {
      "amount": 1500
    }
  }'
```

**核验冲突响应示例**：
```json
{
  "status": "conflict",
  "summary": {
    "total_checked": 1,
    "matched_count": 0,
    "conflict_count": 1,
    "missing_count": 0
  },
  "fields": {
    "/amount": {
      "status": "conflict",
      "system_value": 1500,
      "document_value": 1600,
      "diff_reason": "Discrepancy: system recorded 1500, but document states 1600",
      "evidence": [
        {
          "page": 1,
          "item_ids": ["p1_i0008"],
          "quote": "费用价格：总金额为人民币 1600 元 税前。",
          "bbox": [50.00, 270.00, 290.00, 282.50]
        }
      ]
    }
  },
  "meta": {
    "result_hash": "3d9c8172...",
    "model": "Qwen3-8B"
  }
}
```

---

### 异步任务工作流 (Async Workflow)
对于长篇复杂任务，所有核心接口均支持在 Query 中添加 `?async=true`：
```bash
# 提交任务，立即返回 202 Accepted
curl -X POST "http://localhost:8080/v1/extract?async=true" -d '{ ... }'
# 返回: { "job_id": "job_01h...", "status": "pending", "status_url": "/v1/jobs/job_01h..." }

# 轮询状态
curl http://localhost:8080/v1/jobs/job_01h...
```

---

## 📐 开放规范文档 (Specifications)

Duon 始终坚持“规格优先（Spec-First）”，所有核心数据契约独立于代码存在：

- 📜 [Document IR 设计哲学与效果提升原理（为什么比原生解析器更好）](docs/document-ir-design-rationale.md)
- 📊 [LiteParse 原生格式 vs Duon Document IR 基准测试方案](docs/benchmark-plan-liteparse-vs-document-ir.md)
- 📜 [Document IR 1.0 开放标准协议](specs/protocols/document-ir-spec.v1.md)
- 📜 [OpenAPI 3.1 完整接口规范](specs/openapi/v1-api.yaml)
- 📜 [确定性输出与 Canonical JSON 算法标准](specs/protocols/canonical-json-spec.md)
- 📜 [安全防御与临时文件生命周期规范](specs/protocols/security-and-lifecycle-spec.md)
- 📜 [Document IR 机器校验 Schema (JSON Schema Draft 2020-12)](specs/json-schema/document-ir.v1.json)

---

## 📂 代码目录与架构分包 (Repository Structure)

```text
duon/
├── crates/
│   ├── core/         # 核心事实模型 (Document IR, BBox 几何约束, Canonicalizer)
│   ├── parser/       # 解析层抽象 (DocumentParser trait, LiteParseAdapter, MockParser)
│   ├── extractor/    # 语义理解层 (ModelBackend, ContextBuilder, EvidenceBinder, VerifyEngine)
│   └── api/          # HTTP 接口层 (Axum 路由, SSRF 安全网关, EphemeralFile, JobStore)
├── specs/
│   ├── openapi/      # OpenAPI 3.1 契约 (v1-api.yaml)
│   ├── json-schema/  # 机器可读 JSON Schema (IR, Extract, Verify)
│   ├── protocols/    # 算法与标准规范文档 (Document IR Spec, Canonical JSON, SSRF)
│   └── mocks/        # 标准黄金 Mock 样本集 (sample_order_ir, sample_extract_req 等)
├── Cargo.toml        # Cargo 多 Crate 工作区配置
└── .env.example      # 环境变量运行模版
```

---

## 🤝 参与贡献 (Contributing)

欢迎提交 Issue 和 Pull Request！在提交代码前，请确保：
1. 编写对应的单元测试或集成测试（遵循 TDD 原则）。
2. 执行测试套件并通过：`cargo test`。
3. 执行代码风格检查：`cargo clippy`。

---

## 📄 开源许可证 (License)

本项目遵循 **MIT 或 Apache-2.0** 双重开源协议许可证。详情请参阅项目内许可声明。
