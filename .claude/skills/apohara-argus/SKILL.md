```markdown
# apohara-argus Development Patterns

> Auto-generated skill from repository analysis

## Overview
This skill teaches the core development conventions and patterns used in the `apohara-argus` Rust codebase. You'll learn how to structure files, write imports and exports, follow commit message conventions, and organize tests. This guide is ideal for contributors looking to quickly align with the project's established practices.

## Coding Conventions

### File Naming
- Use **PascalCase** for file names.
  - Example: `MyModule.rs`, `UserService.rs`

### Import Style
- Use **relative imports** within modules.
  - Example:
    ```rust
    use super::SomeHelper;
    use crate::utils::Logger;
    ```

### Export Style
- Use **named exports** for functions, structs, and modules.
  - Example:
    ```rust
    pub struct UserData { /* ... */ }
    pub fn process_user() { /* ... */ }
    ```

### Commit Messages
- Use **conventional commit** style.
- Prefix with `fix` for bug fixes.
- Keep messages concise (average ~65 characters).
  - Example:
    ```
    fix: correct user authentication flow on login
    ```

## Workflows

### Code Fix Workflow
**Trigger:** When fixing a bug or issue in the codebase  
**Command:** `/fix-bug`

1. Identify and fix the bug in the relevant module.
2. Write or update tests in the corresponding `*.test.*` file.
3. Commit changes with a conventional commit message prefixed by `fix`.
   - Example: `fix: resolve panic in data parser`
4. Push your changes and open a pull request.

## Testing Patterns

- **Test File Naming:** Test files follow the `*.test.*` pattern.
  - Example: `UserService.test.rs`
- **Testing Framework:** Not explicitly detected; use standard Rust testing conventions.
- **Test Example:**
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_process_user() {
          // Test logic here
      }
  }
  ```

## Commands
| Command    | Purpose                                   |
|------------|-------------------------------------------|
| /fix-bug   | Start the bug fix workflow                |
```
