---
title: REST API Design Best Practices
author: Developer Guide
tags: [api, rest, http, design]
---

# REST API Design Best Practices

Building well-designed APIs is crucial for developer experience.

## HTTP Methods

| Method | Purpose | Idempotent |
|--------|---------|------------|
| GET    | Retrieve resource | Yes |
| POST   | Create resource | No |
| PUT    | Update resource | Yes |
| DELETE | Remove resource | Yes |
| PATCH  | Partial update | No |

## Resource Naming

Use nouns for resources, not verbs:

```
Good:
GET /users
GET /users/{id}
POST /users
DELETE /users/{id}

Bad:
GET /getUsers
POST /createUser
```

## Error Handling

Return consistent error responses:

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Invalid input",
    "details": [
      {"field": "email", "message": "Invalid email format"}
    ]
  }
}
```

## Pagination

Implement cursor-based pagination for large datasets:

```
GET /users?cursor=abc123&limit=20
```

Response:
```json
{
  "data": [...],
  "pagination": {
    "next_cursor": "xyz789",
    "has_more": true
  }
}
```

## Rate Limiting

Protect APIs with rate limiting headers:

```
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 95
X-RateLimit-Reset: 1609459200
```

## Authentication

Use Bearer tokens for API authentication:

```
Authorization: Bearer eyJhbGciOiJIUzI1NiIs...
```
