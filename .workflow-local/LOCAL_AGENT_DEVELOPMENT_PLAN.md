# Local Agent: Full-Workflow Academic Writing Agent Development Plan

## Context

ChatPrism's local agent runtime currently functions as a general-purpose coding/editing assistant. The user wants to transform it into a **full-workflow biomedical academic writing agent** that participates in literature review, experiment/data analysis, paper writing, and peer review — with Claude-like comprehensive reasoning capabilities across any compatible LLM provider (OpenAI, DeepSeek, MiniMax, etc.).

**Current state**: The agent has a solid foundation — turn engine with budget management, 12 tools across 4 categories, document artifact system (PDF/DOCX), citation search with 3 providers, task-kind routing, and sampling profiles. But it lacks domain-aware prompting, structured academic workflows, and the specialized tools needed for end-to-end biomedical writing.

**Goal**: Incrementally extend the existing runtime into a domain-expert agent that can drive an academic writing session from literature survey through submission-ready manuscript, without being tied to a single LLM provider. No Python/computation for now — focus on text intelligence.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Frontend (React)                         │
│  agent-chat-store.ts ← workflow panel ← resource sidebar        │
├─────────────────────────────────────────────────────────────────┤
│                     Tauri Command Layer                          │
│  agent_run_turn / agent_run_workflow / citation_search           │
├────────────┬──────────┬─────────────┬──────────────────────────┤
│ Workflow   │ Domain   │ Tool System │ Provider Abstraction      │
│ Orchestrator│ Prompts  │ (expanded)  │ (OpenAI/DS/MM/...)       │
├────────────┴──────────┴─────────────┴──────────────────────────┤
│              Turn Engine (existing, extended)                    │
│  TurnBudget · parallel batching · approval gates · suspension   │
├─────────────────────────────────────────────────────────────────┤
│  Citation Search │ Document Artifacts │ Session/State Mgmt      │
└─────────────────────────────────────────────────────────────────┘
```

---

## Phase 1: Domain-Aware Prompt Engineering (Biomedical)

**Objective**: Make the agent a biomedical domain expert through prompt engineering alone — zero tool changes.

### 1.1 Domain System Prompt Layer

**File**: `src-tauri/src/agent/mod.rs` — new constant `BIOMEDICAL_DOMAIN_INSTRUCTIONS`

```
位置: 在 AGENT_BASE_INSTRUCTIONS 之后拼接, 由 build_agent_instructions_with_work_state() 注入
```

Content covers:
- **Terminology precision**: Use correct biomedical nomenclature (gene/protein naming per HUGO, disease per MeSH/ICD, chemical per IUPAC). Never fabricate citations or data.
- **Evidence hierarchy**: Systematic review > RCT > cohort > case-control > case series > expert opinion. When analyzing literature, always identify study design and evidence level.
- **Statistical awareness**: Recognize p-values, confidence intervals, effect sizes, power analysis. Flag when claims lack statistical support.
- **Citation style**: Default to author-year for biomedical (Nature, Cell, PNAS conventions). Understand numbered styles (Vancouver) when requested.
- **Ethical awareness**: Flag potential conflicts of interest, IRB/ethics considerations, CONSORT/PRISMA checklist compliance.

### 1.2 Task-Kind Expansion

**File**: `src-tauri/src/agent/provider.rs`

Extend `AgentTaskKind` enum:

```rust
pub enum AgentTaskKind {
    General,
    SelectionEdit,
    FileEdit,
    SuggestionOnly,
    Analysis,
    // New academic task kinds:
    LiteratureReview,    // Phase 2
    PaperDrafting,       // Phase 3
    PeerReview,          // Phase 4
}
```

Each new task kind gets:
- Dedicated `resolve_turn_profile()` routing rules in `mod.rs`
- Sampling profile mapping (e.g., LiteratureReview → AnalysisDeep)
- Max rounds budget (LiteratureReview: 15, PaperDrafting: 12, PeerReview: 10)
- Task-specific instruction appendix injected by `build_agent_instructions_with_work_state()`

### 1.3 Intent Detection for Academic Tasks

**File**: `src-tauri/src/agent/mod.rs` — new detection functions

```rust
fn prompt_requests_literature_review(prompt: &str) -> bool { ... }
fn prompt_requests_paper_drafting(prompt: &str) -> bool { ... }
fn prompt_requests_peer_review(prompt: &str) -> bool { ... }
```

Keyword patterns (bilingual EN/ZH):
- Literature: "review literature", "find papers", "what does the literature say", "文献综述", "找文献", "相关研究"
- Drafting: "write introduction", "draft methods", "write discussion", "写引言", "撰写方法", "草拟讨论"
- Review: "review this manuscript", "check for issues", "审稿", "审查论文", "找问题"

Updated `resolve_turn_profile()` integrates these into automatic task classification.

### 1.4 Settings Integration

**File**: `src-tauri/src/settings/types.rs`

```rust
pub struct AgentDomainConfig {
    pub domain: String,           // "biomedical", "general", "chemistry", ...
    pub custom_instructions: Option<String>,  // user-supplied additions
    pub terminology_strictness: String,       // "strict" | "moderate" | "relaxed"
}
```

UI: Add "Domain" selector to AI Assistant tab (dropdown: General / Biomedical / Chemistry / Custom).

---

## Phase 2: Literature Review & Analysis Workflow

**Objective**: Agent can autonomously search, read, analyze, and synthesize literature.

### 2.1 Enhanced Citation Search for Biomedical

**File**: `src-tauri/src/citation/query.rs`

- Add PubMed/MEDLINE as 4th citation provider (API: E-utilities, free, no key required for <3 req/s)
- MeSH term expansion: map user queries to MeSH descriptors for structured PubMed search
- Boolean query builder: support AND/OR/NOT for multi-concept searches
- Date range filtering (common in biomedical: "papers from last 5 years")

**File**: `src-tauri/src/citation/providers.rs` — new `pubmed.rs` module

```rust
pub struct PubMedProvider;
impl CitationProvider for PubMedProvider {
    async fn search(&self, queries: &[QueryPlanItem], limit: u32) -> Vec<RawCitationHit>;
}
```

PubMed-specific features:
- ESearch → EFetch pipeline (IDs first, then metadata)
- PMID as canonical identifier
- MeSH qualifier extraction for scoring
- Abstract retrieval for relevance ranking

### 2.2 Literature Analysis Tools

**File**: `src-tauri/src/agent/tools/literature.rs` (new)

| Tool | Description |
|------|-------------|
| `search_literature` | Wraps citation search with academic-optimized query building. Returns structured results with title, authors, year, journal, abstract, DOI. |
| `analyze_paper` | Given an attached PDF/document, extract structured info: objective, methods, key findings, limitations, relevance score. |
| `compare_papers` | Cross-reference 2+ attached papers: shared/conflicting findings, methodological differences, complementary evidence. |
| `synthesize_evidence` | Given multiple search results or papers, produce a narrative synthesis organized by themes/questions. |
| `extract_methodology` | From a paper, extract detailed methodology: study design, sample size, interventions, endpoints, statistical methods. |

Tool registration: Add to `tools.rs` tool dispatch table, assign `ToolCapabilityClass::LiteratureAnalysis`.

### 2.3 Literature Review Workflow Orchestrator

**File**: `src-tauri/src/agent/workflows/literature_review.rs` (new)

A multi-turn orchestrated workflow (not a single tool call):

```
Step 1: Query Understanding
  - Parse user's research question
  - Identify key concepts, population, intervention, comparison, outcome (PICO)
  - Generate structured search strategy

Step 2: Systematic Search
  - Execute across providers (S2 + OpenAlex + Crossref + PubMed)
  - De-duplicate by DOI/PMID
  - Score and rank by relevance

Step 3: Screen & Filter
  - Apply inclusion/exclusion criteria (if user specified)
  - Prioritize by evidence level and recency
  - Present candidate list for user review

Step 4: Deep Analysis
  - For selected papers (attached PDFs), run analyze_paper
  - Extract key data into structured table

Step 5: Synthesis
  - Produce narrative literature review draft
  - Organize by themes or chronologically
  - Insert citations in user's preferred style
  - Flag gaps in the evidence
```

This runs as a **guided multi-turn conversation**, not a single autonomous execution. Each step emits status events and waits for user confirmation before proceeding to the next.

### 2.4 Frontend: Literature Review Panel

**File**: `src/components/agent/LiteratureReviewPanel.tsx` (new)

- PICO form for structured research question input
- Search results table with selection checkboxes
- Evidence matrix view (papers × extracted data points)
- "Generate synthesis" button that triggers Step 5

---

## Phase 3: Paper Writing & Editing Workflow

**Objective**: Agent assists in drafting, structuring, and polishing manuscripts section by section.

### 3.1 Manuscript Structure Awareness

**File**: `src-tauri/src/agent/mod.rs` — section-specific prompt appendices

The agent must understand standard biomedical manuscript structure:
- **IMRaD**: Introduction, Methods, Results, and Discussion
- **Sections**: Title, Abstract (structured vs unstructured), Keywords, Acknowledgments, References
- **Supplementary**: Figures, Tables, Supplementary Materials

When working on a specific section, inject section-specific instructions:
- Introduction: background → gap → objective flow; cite seminal + recent works
- Methods: reproducibility focus; protocol detail; statistical plan
- Results: objective reporting; figures/tables referenced in order; no interpretation
- Discussion: findings → context → limitations → implications → future directions

### 3.2 Writing Tools

**File**: `src-tauri/src/agent/tools/writing.rs` (new)

| Tool | Description |
|------|-------------|
| `draft_section` | Generate a section draft given: section type, key points, tone, word count target, citation keys to incorporate. Output is LaTeX or Markdown. |
| `restructure_outline` | Given a manuscript or outline, propose restructured organization with rationale. |
| `insert_citation` | Insert a citation at cursor using project's bibliography. Integrates with citation search. |
| `check_consistency` | Scan manuscript for internal inconsistencies: terminology, abbreviations, figure/table numbering, reference completeness. |
| `generate_abstract` | From full manuscript text, generate structured abstract (Background/Methods/Results/Conclusions) within word limit. |

### 3.3 Writing Quality Prompts

Section-specific quality checks injected into prompt when task_kind is PaperDrafting:
- **Hedging language**: appropriate use of "may", "suggests", "indicates" vs. overclaiming
- **Active/passive voice**: biomedical convention (Methods typically passive, Discussion can be active)
- **Abbreviation management**: define on first use, maintain consistency list
- **Transition coherence**: logical flow between paragraphs
- **Signposting**: clear topic sentences, explicit connections to research question

### 3.4 Template System

**File**: `src-tauri/src/agent/templates/` (new directory)

Pre-built manuscript templates as structured JSON:
- `imrad_standard.json` — standard research article
- `review_article.json` — narrative/systematic review
- `case_report.json` — clinical case report
- `methods_paper.json` — methodology-focused article

Each template defines:
```json
{
  "name": "IMRaD Standard",
  "sections": [
    { "id": "title", "label": "Title", "guidance": "...", "word_target": null },
    { "id": "abstract", "label": "Abstract", "guidance": "...", "word_target": 250 },
    { "id": "introduction", "label": "Introduction", "guidance": "...", "word_target": 800 },
    ...
  ],
  "citation_style": "author-year",
  "domain_defaults": { "terminology_strictness": "strict" }
}
```

Frontend can display template as outline sidebar, letting user navigate and request drafting per-section.

---

## Phase 4: Peer Review & Revision Workflow

**Objective**: Agent can systematically review manuscripts (own or others') and assist with revisions.

### 4.1 Review Criteria Framework

**File**: `src-tauri/src/agent/workflows/peer_review.rs` (new)

Structured review dimensions:
1. **Scientific rigor**: Study design appropriateness, statistical validity, reproducibility
2. **Novelty**: Incremental vs. significant contribution, relation to existing work
3. **Clarity**: Writing quality, logical flow, figure quality
4. **Completeness**: Missing controls, unreported outcomes, incomplete methods
5. **Ethics**: Appropriate approvals, conflict declarations, patient consent

Output format: structured review report with severity ratings (Critical / Major / Minor / Suggestion) per finding.

### 4.2 Review Tools

**File**: `src-tauri/src/agent/tools/review.rs` (new)

| Tool | Description |
|------|-------------|
| `review_manuscript` | Full manuscript review against configurable criteria. Returns structured findings. |
| `check_statistics` | Examine reported statistics for common errors: wrong test selection, unreported assumptions, p-hacking indicators. |
| `verify_references` | Cross-check cited references exist, are correctly attributed, and support the claims made. Uses citation search providers. |
| `generate_response_letter` | Given reviewer comments + manuscript, draft point-by-point response letter with revision plan. |
| `track_revisions` | Compare two versions of a section, summarize changes, verify all reviewer points addressed. |

### 4.3 Review Workflow Orchestrator

```
Step 1: Manuscript Ingestion
  - Parse attached manuscript (PDF/DOCX → document artifact)
  - Identify sections, extract structure

Step 2: Section-by-Section Review
  - For each section, apply relevant review criteria
  - Cross-reference claims against cited literature
  - Check internal consistency

Step 3: Statistical Review
  - Extract all reported statistics
  - Check test appropriateness for data types
  - Verify consistency between text, tables, figures

Step 4: Generate Review Report
  - Organize findings by severity
  - Provide specific, actionable suggestions
  - Include positive observations (not just criticism)

Step 5: Revision Assistance (if user's own paper)
  - Map reviewer comments to manuscript locations
  - Suggest specific text changes
  - Draft response letter paragraphs
```

### 4.4 Checklist System

Pre-built review checklists that can be loaded as context:
- CONSORT (randomized trials)
- PRISMA (systematic reviews)
- STROBE (observational studies)
- ARRIVE (animal research)
- CARE (case reports)

Agent checks manuscript against applicable checklist and reports compliance gaps.

---

## Phase 5: Multi-Model Intelligence Layer

**Objective**: Ensure all workflows perform well across different LLM providers, not just one.

### 5.1 Provider-Adaptive Prompt Engineering

**File**: `src-tauri/src/agent/mod.rs`

Different models have different strengths. The prompt layer should adapt:

```rust
fn adapt_instructions_for_provider(base: &str, provider: &str) -> String {
    match provider {
        "openai" => {
            // GPT-4+ handles complex multi-step well, can be more concise
            format!("{base}\n[Provider note: You can use structured output. Prefer JSON when returning data.]")
        }
        "deepseek" => {
            // DeepSeek-V3 good at reasoning, explicit chain-of-thought helps
            format!("{base}\n[Provider note: Think step by step. Show your reasoning before conclusions.]")
        }
        "minimax" => {
            // MiniMax good at Chinese, explicit bilingual handling
            format!("{base}\n[Provider note: 可以使用中文输出分析结果。When user writes in Chinese, respond in Chinese.]")
        }
        _ => base.to_string()
    }
}
```

### 5.2 Tool Schema Compatibility

Current tool schemas use OpenAI function-calling format. Ensure:
- DeepSeek (chat completions) receives tools in `tools` array format
- MiniMax receives compatible tool definitions
- Response parsing handles provider-specific tool call formats

**File**: `src-tauri/src/agent/chat_completions.rs` and `src-tauri/src/agent/openai.rs`

Add provider-specific tool schema adapters if any provider deviates from the OpenAI tool-calling standard.

### 5.3 Fallback & Retry Logic

**File**: `src-tauri/src/agent/turn_engine.rs`

- If a tool call fails to parse from a provider's response, retry with explicit formatting instructions
- If a provider doesn't support tool_choice="required", fall back to strong prompting
- Token budget awareness: different models have different context windows; TurnBudget should read model capabilities

---

## Phase 6: Session & State Management Enhancements

**Objective**: Support long-running academic workflows that span multiple sessions.

### 6.1 Workflow State Persistence

**File**: `src-tauri/src/agent/session.rs`

Extend `AgentSessionWorkState` to track:
```rust
pub struct AcademicWorkflowState {
    pub workflow_type: Option<String>,     // "literature_review" | "paper_drafting" | "peer_review"
    pub current_step: Option<String>,      // e.g., "search", "screen", "synthesize"
    pub collected_references: Vec<CollectedReference>,
    pub manuscript_outline: Option<ManuscriptOutline>,
    pub review_findings: Vec<ReviewFinding>,
    pub revision_tracker: Option<RevisionTracker>,
}
```

This persists across sessions so the user can:
- Start a literature review, close the app, resume later
- Accumulate references across multiple search sessions
- Track revision progress point by point

### 6.2 Reference Collection

```rust
pub struct CollectedReference {
    pub doi: Option<String>,
    pub pmid: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<u32>,
    pub journal: Option<String>,
    pub abstract_text: Option<String>,
    pub user_notes: Option<String>,
    pub relevance_tag: Option<String>,  // "high" | "medium" | "low"
    pub added_at: String,  // ISO timestamp
}
```

Agent can reference this collection when drafting ("use the 15 papers I collected earlier").

---

## Implementation Roadmap

### Sprint 1 (Foundation) — ~1 week
1. Add `BIOMEDICAL_DOMAIN_INSTRUCTIONS` to `mod.rs`
2. Add new `AgentTaskKind` variants (LiteratureReview, PaperDrafting, PeerReview)
3. Add academic intent detection functions
4. Update `resolve_turn_profile()` for new task kinds
5. Add domain selector to Settings UI (AI Assistant tab)
6. **Verify**: Existing agent functionality unchanged; new task kinds route correctly

### Sprint 2 (Literature) — ~2 weeks
1. Implement PubMed provider in `citation/providers/`
2. Build `tools/literature.rs` (search_literature, analyze_paper)
3. Implement literature review workflow orchestrator (guided multi-turn)
4. Wire new tools into tool dispatch
5. Frontend: basic literature review interaction in chat
6. **Verify**: End-to-end: ask "find papers about CRISPR-Cas9 in oncology" → get structured results

### Sprint 3 (Writing) — ~2 weeks
1. Build `tools/writing.rs` (draft_section, check_consistency, generate_abstract)
2. Section-specific prompt appendices for IMRaD
3. Manuscript template system (JSON templates)
4. Frontend: section-aware editing mode
5. **Verify**: Attach a partial manuscript → agent drafts missing Discussion section with citations

### Sprint 4 (Review) — ~2 weeks
1. Build `tools/review.rs` (review_manuscript, check_statistics, generate_response_letter)
2. Peer review workflow orchestrator
3. Reporting checklist system (CONSORT, PRISMA, etc.)
4. Revision tracking integration
5. **Verify**: Attach a manuscript PDF → agent produces structured review with severity ratings

### Sprint 5 (Polish) — ~1 week
1. Provider-adaptive prompt engineering
2. Tool schema compatibility testing across all 3 providers
3. Workflow state persistence
4. Reference collection across sessions
5. End-to-end integration testing: full workflow from literature → draft → review

---

## File Change Summary

| File / Directory | Action | Phase |
|---|---|---|
| `agent/mod.rs` | Modify: domain instructions, intent detection, task routing | 1 |
| `agent/provider.rs` | Modify: new AgentTaskKind variants | 1 |
| `agent/tools.rs` | Modify: register new tool categories | 2-4 |
| `agent/tools/literature.rs` | **Create** | 2 |
| `agent/tools/writing.rs` | **Create** | 3 |
| `agent/tools/review.rs` | **Create** | 4 |
| `agent/workflows/` | **Create directory** | 2-4 |
| `agent/workflows/literature_review.rs` | **Create** | 2 |
| `agent/workflows/peer_review.rs` | **Create** | 4 |
| `agent/templates/` | **Create directory** with JSON templates | 3 |
| `agent/session.rs` | Modify: academic workflow state | 6 |
| `agent/turn_engine.rs` | Modify: provider fallback, budget for new tasks | 5 |
| `citation/providers.rs` | Modify: add PubMed provider | 2 |
| `citation/providers/pubmed.rs` | **Create** | 2 |
| `settings/types.rs` | Modify: AgentDomainConfig | 1 |
| `src/components/workspace/settings-tabs/AIAssistantTab.tsx` | Modify: domain selector | 1 |
| `src/components/agent/LiteratureReviewPanel.tsx` | **Create** (optional, Sprint 2) | 2 |

**Unchanged**: turn_engine core loop, existing 12 tools, document_artifacts, chat_completions/openai providers, settings schema/store, frontend agent-chat-store (extended, not rewritten).

---

## Verification Plan

```bash
# After each sprint:

# 1. Rust compilation
cd apps/desktop && cargo check

# 2. TypeScript compilation (if frontend touched)
cd apps/desktop && npx tsc --noEmit

# 3. Functional testing per sprint:
# Sprint 1: Open chat, type biomedical prompt → verify task_kind routes to LiteratureReview
# Sprint 2: "Find papers about CRISPR therapy in leukemia" → get structured citation results
# Sprint 3: Attach outline → "Draft the Introduction" → get section with citations
# Sprint 4: Attach manuscript PDF → "Review this paper" → get structured review report
# Sprint 5: Test same workflows with OpenAI/DeepSeek/MiniMax providers
```

---

## Key Design Decisions

1. **Incremental, not monolithic**: Each phase is independently useful. Sprint 1 alone makes the agent better at biomedical tasks through prompting.
2. **Tools over automation**: New capabilities are exposed as tools the LLM can call, not rigid pipelines. This preserves the flexible reasoning that makes Claude-like interaction possible.
3. **Workflows as guided conversations**: Multi-step workflows (literature review, peer review) run as structured multi-turn exchanges with user checkpoints, not black-box autonomous pipelines.
4. **Provider-agnostic by design**: All intelligence lives in prompts and tool definitions, not provider-specific APIs. Provider adaptation is a thin shim layer.
5. **No Python requirement**: All analysis is text-based using the LLM's reasoning. Computational analysis can be added later via the existing `uv.rs` integration.
6. **Reuse existing infrastructure**: Document artifacts for PDF/DOCX, citation search for literature, turn engine for execution, session system for persistence.
