-- Validated monetary observations and EDINET correction/withdrawal audit.
CREATE TABLE IF NOT EXISTS reit_rag_numbers (
 document_id TEXT PRIMARY KEY REFERENCES reit_rag_documents(id),
 body TEXT NOT NULL CHECK(json_valid(body))
);
CREATE TABLE IF NOT EXISTS reit_rag_document_events (
 id TEXT PRIMARY KEY, doc_id TEXT NOT NULL, parent_doc_id TEXT,
 observed_at INTEGER NOT NULL, listed_on TEXT NOT NULL,
 body TEXT NOT NULL CHECK(json_valid(body))
);
CREATE INDEX IF NOT EXISTS reit_rag_event_document ON reit_rag_document_events(doc_id,parent_doc_id);
