# Plan: Java Standard Library Nullness Database

## Objective
Handle nullness of fields, arguments, and return values of Java standard APIs based on the database maintained by the Checker Framework project.

## Background
The Java standard library does not provide nullness annotations by default. The Checker Framework project maintains a comprehensive database of nullness information for Java standard APIs, which we can leverage to improve nullness analysis accuracy.

## Checker Framework Stubs
- Repository: https://github.com/typetools/checker-framework
- Stubs location: `checker/jdk/nullness/src/`
- License: **MIT License** (compatible with AGPL-3.0)
- Covers: Java standard library APIs with nullness annotations

## Implementation Approach

### 1. License Compliance
- Verify Checker Framework license (MIT) compatibility with inspequte (AGPL-3.0)
- Add proper attribution in LICENSE or NOTICE file
- Document the source of nullness data

### 2. Data Extraction
Options for incorporating the database:
- **Option A**: Parse Checker Framework stub files at build time
  - Extract nullness annotations from stub files
  - Generate Rust data structures (e.g., using build.rs)
  - Embed as static data in the binary

- **Option B**: Manual curation
  - Create our own database based on Checker Framework stubs
  - Maintain in a structured format (JSON/YAML)
  - Update periodically to track Java releases

- **Option C**: Runtime parsing
  - Ship stub files with inspequte
  - Parse on-demand during analysis
  - Higher runtime overhead but easier updates

**Recommended**: Option A (build-time extraction)

### 3. Data Structure
```rust
struct StandardLibraryNullness {
    // Map: "java/lang/String" -> ClassNullnessInfo
    classes: BTreeMap<String, ClassNullnessInfo>,
}

struct ClassNullnessInfo {
    fields: BTreeMap<String, Nullness>,
    methods: BTreeMap<MethodSignature, MethodNullnessInfo>,
}

struct MethodNullnessInfo {
    return_nullness: Nullness,
    param_nullness: Vec<Nullness>,
}
```

### 4. Integration with Nullness Rule
- Modify `NullnessRule` to query the database
- Use database info when analyzing calls to standard library methods
- Provide fallback behavior when database has no information

### 5. Java Version Support
- Assume project depends on latest LTS release (Java 25 for now)
- No need to handle multiple Java versions
- Update database when new LTS releases arrive

## Implementation Steps
1. Review Checker Framework stub files for coverage and format
2. Create build.rs script to parse stub files
3. Generate Rust code for the nullness database
4. Integrate database lookup into `NullnessRule::run()`
5. Add tests using standard library methods:
   - `String.charAt()` (non-null return)
   - `Map.get()` (nullable return)
   - `List.add()` (non-null parameter)
   - `Collections.emptyList()` (non-null return)

## Technical Considerations
- Stub files use Java syntax with annotations
- May need Java parser or custom stub parser
- Database size impact on binary size
- Update frequency and maintenance process
- Handling of overloaded methods

## Success Criteria
- Successfully extract nullness information from Checker Framework stubs
- Database covers common Java standard library APIs:
  - `java.lang.*`
  - `java.util.*`
  - `java.io.*`
  - `java.nio.*`
- Nullness analysis correctly uses database information
- Proper license attribution included
- Documentation on updating the database

## Dependencies
- Checker Framework stubs (MIT License)
- Java parser library (possibly as build dependency)
- Current nullness rule implementation

## License Attribution
Add to LICENSE or create NOTICE file:
```
This project includes nullness annotation data derived from the Checker Framework project
(https://github.com/typetools/checker-framework), which is licensed under the MIT License.
Copyright (c) the Checker Framework developers.
```

## Estimated Complexity
**Medium-High** - Requires build-time code generation, parser for stub files, and integration with existing analysis.
