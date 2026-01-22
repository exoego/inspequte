# SLF4J_ILLEGAL_PASSED_CLASS

## Goal
Detect illegal class objects passed to `LoggerFactory.getLogger(Class)`.

## Detection approach
- Match `org/slf4j/LoggerFactory.getLogger(Ljava/lang/Class;)`.
- Report when the argument is a different class literal than the enclosing class
  or any of its outer classes.
- Allow `getClass()` and `EnclosingClass.class` or its outers.

## Bytecode signals
- INVOKESTATIC `org/slf4j/LoggerFactory.getLogger` with a `Class` parameter.
- LDC of a class literal (`Foo.class`) and ALOAD_0 + INVOKEVIRTUAL `java/lang/Object.getClass()`.

## Tests
- Report: `LoggerFactory.getLogger(Bar.class)` inside `Foo`.
- Allow: `LoggerFactory.getLogger(getClass())`.
- Allow: `LoggerFactory.getLogger(Foo.class)`.
- Allow: `LoggerFactory.getLogger(Outer.class)` inside `Outer.Inner`.

## Edge cases
- Nested classes: allow `Outer.class` inside `Outer.Inner`.
- Unknown arg sources should not trigger.
