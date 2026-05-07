---
name: markdown-writing
description: Write markdown documents (README, guides, wikis, changelogs, API docs) by breaking them into small sequential chunks instead of generating one long file at a time. Use when the user needs structured markdown output that is long, multi-section, or likely to exceed a single generation window.
origin: harness
---

# Markdown Writing

Write markdown documents in small, verifiable chunks — never as one long generation.

## When to Activate

- writing or updating README files
- creating guides, wikis, tutorials, or changelogs
- producing API documentation or architecture docs
- converting notes or specs into structured markdown
- any markdown output longer than ~100 lines

## The Core Problem

Long markdown documents fail when generated all at once:
- output gets truncated mid-section
- structure drifts or becomes inconsistent
- errors in one section corrupt the rest
- hard to review or correct specific parts

**Solution: treat a document like code — break it into small, independent sections and write each one separately.**

## Chunking Strategy (Required)

### Step 1: Build the Outline First

Before writing any content, produce the full outline and get confirmation:

```
## Outline

1. Title + badges (if README)
2. Overview / Introduction
3. Prerequisites
4. Installation
5. Usage
6. Configuration
7. API Reference
8. Contributing
9. License
```

### Step 2: Write One Section at a Time

- Write exactly one section per generation
- Announce which section: `### Writing: Section 3 — Installation`
- End each section with a boundary marker: `<!-- section: installation — done -->`
- Wait for user confirmation before moving to the next section

### Step 3: Assemble at the End

Only after all sections are confirmed, assemble using `fsWrite` for the first chunk and `fsAppend` for every subsequent section.

**Never use a single `fsWrite` call for a document longer than 50 lines.**

## Section Size Limits

| Section type | Max lines per chunk |
|---|---|
| Title + badges | 15 |
| Overview / intro | 40 |
| Installation / setup | 60 |
| Usage examples | 60 |
| API reference (per endpoint) | 50 |
| Configuration table | 50 |
| Any other section | 60 |

If a section exceeds its limit, split into sub-sections and write each separately.

## File Writing Rules

- Use `fsWrite` only for the first chunk
- Use `fsAppend` for every section after the first
- Never pipe or echo content through terminal commands
- Never use `cat <<EOF` to create files

## Markdown Quality Rules

1. Every code block must specify a language: ` ```bash `, ` ```json `, ` ```typescript `
2. Tables must have a header row and alignment row
3. Links must be verified or marked as placeholders: `[text](URL_HERE)`
4. Headings must follow hierarchy — never skip from `##` to `####`
5. One blank line before and after every code block, table, and list
6. No trailing spaces

## README-Specific Rules

- Always include a badges section (use `readme-rule` skill if available)
- Structure: Title → Badges → Overview → Install → Usage → Config → Contributing → License
- Keep the "Overview" under 5 sentences
- Every install step must be a runnable command in a code block

## Changelog-Specific Rules

- Follow [Keep a Changelog](https://keepachangelog.com) format
- Group entries: `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`
- Most recent version at the top
- Each entry is one line, starts with a verb in past tense

## Quality Gate

Before delivering any section:
- [ ] Heading hierarchy is correct
- [ ] All code blocks have a language tag
- [ ] No placeholder text left unfilled (`TODO`, `FIXME`, `your-value-here`)
- [ ] Section boundary marker appended

Before final assembly:
- [ ] All sections confirmed by user
- [ ] `fsWrite` used only for first chunk
- [ ] `fsAppend` used for all subsequent chunks
- [ ] Full document renders correctly in preview
