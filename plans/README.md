# Plans Directory

This directory stores cross-cutting plans for inspequte.

Rule-specific plans are now colocated with each rule under:

```text
src/rules/<rule-id>/plan.md
```

## Purpose

Each plan file should document:
- **Objective**: What we want to achieve
- **Background**: Context and motivation
- **Implementation approach**: Technical details and strategy
- **Test cases**: Expected behavior and edge cases
- **Success criteria**: How to verify completion
- **Dependencies**: Required resources and prerequisites
- **Complexity estimate**: Effort level assessment

## Plans In This Directory

1. **[01-type-use-nullness-annotations.done.md](01-type-use-nullness-annotations.done.md)**
   - Extend nullness rule to support type-use annotations like `List<@Nullable Object>`
   - Complexity: **High**
   - Status: **Done**

2. **[02-java-stdlib-nullness-database.md](02-java-stdlib-nullness-database.md)**
   - Handle nullness of Java standard library APIs
   - Use Checker Framework's nullness database (MIT License)
   - Complexity: **Medium-High**
   - Status: **Planning**

3. **[03-file-based-classpath-input.done.md](03-file-based-classpath-input.done.md)**
   - Accept `--input` and `--classpath` values from files using `@file.txt` syntax
   - Complexity: **Low-Medium**
   - Status: **Done**

4. **[04.improve-agent-documentation.done.md](04.improve-agent-documentation.done.md)**
   - Update AGENTS guidance for test harness naming
   - Complexity: **Low**
   - Status: **Done**

## Plan Status

Open cross-cutting work in this directory:
- `02-java-stdlib-nullness-database.md`

Implementation priority is determined by:
- User requests and feedback
- Impact on analysis quality
- Implementation complexity
- Dependencies on other features

## Contributing

When creating a new cross-cutting plan in this directory:
1. Use a descriptive filename with a numeric prefix: `NN-feature-name.md`
2. Include all standard sections: Objective, Background, Implementation, Tests, Success Criteria
3. Estimate complexity: Low, Medium, High, or combinations
4. List all dependencies and prerequisites
5. Consider edge cases and false positives

When implementing a cross-cutting plan in this directory:
1. Rename the plan file with a `.done.md` suffix after implementation is complete and merged
2. Add a short post-mortem section (what went well, what was tricky, follow-ups)

When implementing a rule-specific plan:
1. Keep the file as `src/rules/<rule-id>/plan.md`
2. Add a short post-mortem section in that file when the work is complete

## License Considerations

Some plans involve third-party resources:
- Plan 02 uses Checker Framework stubs (MIT License - compatible with AGPL-3.0)
- Always verify license compatibility before incorporating external data
- Add proper attribution when using third-party resources
