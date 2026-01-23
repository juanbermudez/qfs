# Python Basics

Python is a high-level, interpreted programming language known for its readability and simplicity.

## Variables and Types

Python uses dynamic typing:

```python
# Variables
name = "Alice"
age = 30
is_student = True
grades = [95, 87, 91]
```

## Functions

Functions are defined using the `def` keyword:

```python
def greet(name):
    """Return a greeting message."""
    return f"Hello, {name}!"

result = greet("Bob")
print(result)  # Output: Hello, Bob!
```

## Classes

Python supports object-oriented programming:

```python
class Dog:
    def __init__(self, name, breed):
        self.name = name
        self.breed = breed

    def bark(self):
        return f"{self.name} says woof!"

my_dog = Dog("Max", "Labrador")
print(my_dog.bark())
```

## File Handling

Reading and writing files is straightforward:

```python
# Writing to a file
with open('example.txt', 'w') as f:
    f.write('Hello, World!')

# Reading from a file
with open('example.txt', 'r') as f:
    content = f.read()
```
