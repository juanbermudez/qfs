# Web Development Fundamentals

Modern web development encompasses frontend, backend, and full-stack development.

## Frontend Technologies

### HTML Structure

HTML provides the structure of web pages:

```html
<!DOCTYPE html>
<html>
<head>
    <title>My Page</title>
</head>
<body>
    <h1>Welcome</h1>
    <p>This is a paragraph.</p>
</body>
</html>
```

### CSS Styling

CSS handles visual presentation:

```css
body {
    font-family: Arial, sans-serif;
    margin: 0;
    padding: 20px;
}

.container {
    max-width: 1200px;
    margin: 0 auto;
}
```

### JavaScript Interactivity

JavaScript adds dynamic behavior:

```javascript
document.addEventListener('DOMContentLoaded', () => {
    const button = document.querySelector('#submit');
    button.addEventListener('click', () => {
        console.log('Button clicked!');
    });
});
```

## Backend Development

Backend technologies handle server-side logic:

- **Node.js**: JavaScript runtime
- **Python/Django**: Web framework
- **Rust/Actix**: High-performance APIs
- **Go**: Efficient microservices

## Databases

Common database choices:

- **PostgreSQL**: Relational database
- **MongoDB**: Document database
- **Redis**: In-memory key-value store
- **SQLite**: Embedded database
