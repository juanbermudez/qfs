# TypeScript Design Patterns

Modern TypeScript patterns for building scalable applications.

## Type-Safe Patterns

### Factory Pattern

```typescript
interface Product {
  name: string;
  price: number;
}

function createProduct(name: string, price: number): Product {
  return { name, price };
}
```

### Builder Pattern

```typescript
class QueryBuilder {
  private query: string[] = [];

  select(fields: string[]): this {
    this.query.push(`SELECT ${fields.join(', ')}`);
    return this;
  }

  from(table: string): this {
    this.query.push(`FROM ${table}`);
    return this;
  }

  build(): string {
    return this.query.join(' ');
  }
}
```

## Generic Types

TypeScript generics provide powerful type abstraction:

```typescript
function identity<T>(arg: T): T {
  return arg;
}

interface Repository<T> {
  find(id: string): Promise<T | null>;
  save(entity: T): Promise<void>;
}
```

## Async Patterns

### Promise-based APIs

```typescript
async function fetchUser(id: string): Promise<User> {
  const response = await fetch(`/api/users/${id}`);
  return response.json();
}
```

## Error Handling

TypeScript uses try-catch with typed errors:

```typescript
class ValidationError extends Error {
  constructor(public field: string, message: string) {
    super(message);
  }
}

function validate(input: string): void {
  if (!input) {
    throw new ValidationError('input', 'Input is required');
  }
}
```
