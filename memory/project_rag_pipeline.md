---
name: RAG pipeline project
description: Goal and stack for the rag_personal project — Notion → embeddings → LanceDB → MCP
type: project
---

Personal RAG pipeline in Rust. Stages: ingest Notion data → chunk + embed with fastembed-rs (`intfloat/multilingual-e5-small`) → store in LanceDB → expose retrieval as an MCP server to a personal AI agent.

**Why:** User wants their Notion notes available as retrieval context for their personal agent, running locally.

**How to apply:** Project is greenfield as of 2026-05-13 (only `cargo new` scaffolding). Stack choices are fixed unless user revisits: fastembed-rs, e5-small (multilingual), LanceDB. "Official Notion crate" — note that Notion does not publish an official Rust SDK; community crates like `notion-client` or `notionrs` are the realistic options. Flag this when relevant.
