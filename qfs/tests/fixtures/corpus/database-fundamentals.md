# Database Fundamentals

Understanding databases is essential for building data-driven applications.

## SQL Databases

### PostgreSQL

PostgreSQL is a powerful, open-source relational database:

```sql
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

SELECT * FROM users WHERE email LIKE '%@example.com';
```

### SQLite

SQLite is an embedded database perfect for local storage:

```sql
CREATE TABLE documents (
    id INTEGER PRIMARY KEY,
    title TEXT,
    content TEXT,
    hash TEXT UNIQUE
);
```

## NoSQL Databases

### MongoDB

Document-based storage for flexible schemas:

```javascript
db.users.insertOne({
  name: "Alice",
  email: "alice@example.com",
  preferences: { theme: "dark" }
});
```

## Full-Text Search

### FTS5 in SQLite

SQLite FTS5 provides powerful full-text search:

```sql
CREATE VIRTUAL TABLE documents_fts USING fts5(
    title,
    body,
    content='documents'
);

SELECT * FROM documents_fts WHERE documents_fts MATCH 'search query';
```

### BM25 Ranking

BM25 is a probabilistic ranking function for information retrieval.
It considers term frequency, document length, and inverse document frequency.

## Indexing Strategies

- B-tree indexes for range queries
- Hash indexes for equality lookups
- GIN indexes for full-text search
- Partial indexes for filtered queries
