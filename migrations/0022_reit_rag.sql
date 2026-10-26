-- RAG input and publication history are immutable. Only the publication pointer changes.
CREATE TABLE IF NOT EXISTS reit_rag_documents (
  id TEXT PRIMARY KEY, code TEXT NOT NULL, doc_id TEXT NOT NULL,
  period_end TEXT NOT NULL, submitted TEXT NOT NULL, fetched_at INTEGER NOT NULL,
  content_hash TEXT NOT NULL, original_key TEXT NOT NULL, source_url TEXT NOT NULL,
  parser_version TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('staging','ready'))
);
CREATE INDEX IF NOT EXISTS reit_rag_documents_lookup ON reit_rag_documents(code,period_end,submitted);
CREATE TABLE IF NOT EXISTS reit_rag_chunks (
  id TEXT PRIMARY KEY, document_id TEXT NOT NULL REFERENCES reit_rag_documents(id),
  ordinal INTEGER NOT NULL, location TEXT NOT NULL, body TEXT NOT NULL,
  search_text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS reit_rag_chunks_document ON reit_rag_chunks(document_id,ordinal);
CREATE VIRTUAL TABLE IF NOT EXISTS reit_rag_fts USING fts5(chunk_id UNINDEXED, tokens);
CREATE TABLE IF NOT EXISTS reit_rag_runs (
  id TEXT PRIMARY KEY, cutoff INTEGER NOT NULL, price_date TEXT NOT NULL,
  model TEXT NOT NULL, version TEXT NOT NULL, input_hash TEXT NOT NULL,
  input_key TEXT NOT NULL, output_key TEXT,
  status TEXT NOT NULL CHECK(status IN ('running','ready','failed')),
  error_code TEXT, finished_at INTEGER
);
CREATE TABLE IF NOT EXISTS reit_rag_results (
  run_id TEXT NOT NULL REFERENCES reit_rag_runs(id),
  subject TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('stock','sector')),
  body TEXT NOT NULL CHECK(json_valid(body)), PRIMARY KEY(run_id,kind,subject)
);
CREATE TABLE IF NOT EXISTS reit_rag_publication (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  run_id TEXT NOT NULL REFERENCES reit_rag_runs(id), cutoff INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS reit_rag_vector_documents (
  document_id TEXT NOT NULL, index_name TEXT NOT NULL, embedding_model TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('pending','ready')), first_vector TEXT NOT NULL,
  PRIMARY KEY(document_id,index_name,embedding_model)
);
CREATE TABLE IF NOT EXISTS reit_rag_extraction_cache (
  input_hash TEXT PRIMARY KEY, body TEXT NOT NULL CHECK(json_valid(body)),
  model TEXT NOT NULL, version TEXT NOT NULL, created_at INTEGER NOT NULL
);
