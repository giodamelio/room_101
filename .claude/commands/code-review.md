# /code-review

**Description:** Detailed review of a single Rust file

## Parameters
- `file_path` (string, required): Path to the specific file to review
- `focus` (string, optional, default: "safety,performance,docs,style"): Focus areas for the review

## Prompt

You are conducting a detailed review of a **single Rust file**.

**CAREFUL REVIEW PROCESS:**
1. **FIRST**: Deeply understand the target file and its role in the crate
2. **SECOND**: Carefully analyze the codebase for potential issues
3. **THIRD**: Think critically about each code pattern and design choice
4. **FINALLY**: Document understanding and report only problematic code

**DEEP UNDERSTANDING PHASE:**
- Thoroughly analyze file's purpose and role within the larger crate
- Understand all dependencies and how other modules interact with this file
- Identify key data structures, algorithms, and design patterns
- Map out function relationships and call hierarchies
- Understand the file's position in the overall architecture
- Study the intended behavior vs actual implementation

**CAREFUL ANALYSIS REQUIREMENTS:**
- Think deeply about each function's logic and edge cases
- Consider performance implications of every algorithm choice
- Analyze memory safety and ownership patterns carefully
- Look for subtle bugs and race conditions
- Evaluate API design and usability thoroughly
- Consider maintainability and future evolution

**CAREFUL TARGETED CODE ANALYSIS:**
- Methodically examine each function and data structure (skip simple functions < 4 lines)
- Use deep file/crate understanding to identify subtle problems
- Think critically about performance bottlenecks and optimization opportunities
- Only report code that genuinely needs fixing - ignore good implementations
- Prioritize issues by impact and complexity of fixes
- Focus review effort on complex logic where bugs are more likely

**SYSTEMATIC ISSUE DETECTION - FOCUS ON ACTUAL PROBLEMS:**

## HIGH PRIORITY (Report these):
1. **Real Performance Issues**:
   - Algorithmic complexity problems (O(n²) or worse where O(n) is possible)
   - Unnecessary work in hot paths (e.g., fetching all items then truncating vs limiting at source)
   - Memory leaks or unbounded growth
   - NOT: micro-optimizations, nested matches, error formatting, iterator chains (iter().filter().collect()), "O(n) + O(n)" iterator patterns that the compiler fuses

2. **Actual Logic Errors**:
   - Code that produces incorrect results
   - Genuine panic risks (NOT when expect() has clear explanations)
   - Resource leaks (files, sockets, locks not released)
   - Data races and deadlocks in concurrent code

3. **Security Vulnerabilities**:
   - SQL injection, command injection
   - Unvalidated user input leading to exploits
   - Exposed secrets or credentials
   - NOT: theoretical integer overflow in config modules

## MEDIUM PRIORITY (Consider reporting):
1. **Refactoring Opportunities**:
   - Complex functions that genuinely need splitting (>100 lines with multiple responsibilities) - NOT just nested logic that matches business requirements
   - Inconsistent delegation patterns that create maintenance burden
   - Code duplication that could cause sync issues

2. **Feature Suggestions**:
   - Missing functionality that would improve the system
   - Better error handling strategies
   - Useful validation that's actually missing

## IGNORE (Do NOT report these):
1. **Style Preferences**: Builder patterns, const usage, formatting choices, wildcard imports, module organization
2. **Theoretical Risks**: Panic possibilities when code has expect() with explanations
3. **Common Rust Patterns**: as_any(), downcast_ref(), standard error handling
4. **Config Module Issues**: Integer overflow in configuration, missing validation in defaults
5. **Non-Critical Path Performance**: Error message formatting, debug/trace code
6. **Well-Understood Trade-offs**: When comments explain the design decision
7. **Already Protected Code**: If debug_assert/assert already guards the condition, don't suggest redundant checks
8. **Minor Code Duplication**: Less than 10 lines of duplicated code that maintains readability
9. **Micro-optimizations**: Optimizations that add complexity without proving 10x+ performance improvement
10. **TODO Comments**: Unless you've verified the condition is actually met (use grep/search to check)
11. **Iterator Pattern Preferences**: iter().filter().collect() chains - these are idiomatic Rust patterns that are already optimized by the compiler and more elegant than manual loops
12. **Parallel Collection Operations**: Multiple iterator operations that appear to be "2 passes" (e.g., iter() + filter()) - the Rust compiler optimizes these into single-pass operations through iterator fusion
13. **Complex Nested Logic**: Deeply nested if/match statements or complex control flow - sometimes business logic requires complexity and flattening would reduce readability

**EVALUATION CRITERIA:**
- **Trust the Developer**: If code has comments explaining assumptions, believe them
- **Prove the Problem**: Only report issues you can demonstrate will cause actual failures
- **Consider Context**: Distinguish between critical paths and non-critical paths
- **Understand Production Code**: This is battle-tested code, not a student project
- **Check for Evidence**: Look for existing tests that validate the behavior
- **Avoid Speculation**: Don't report "potential" issues without concrete scenarios
- **Code Maturity**: If code has been in main branch for 6+ months, assume it's correct unless proven otherwise
- **Cost-Benefit Analysis**: Only suggest changes where benefit clearly outweighs complexity cost
- **Iterator Fusion**: Rust's compiler optimizes iterator chains (iter().filter().map().collect()) into single-pass operations - don't flag these as "multiple passes"

**OUTPUT FORMAT:**

**REPORT OUTPUT:**
Create a detailed report at `.report/{file_path}.md` with the following content:

```markdown
# Code Review Report: {file_path}

**Focus Areas:** {focus}
**Have Reviewed By Human:** false

## 🐛 BUGS (Actual problems that will cause failures)

### [BUG-001] {descriptive_issue_name}
- **Location:** {file_path}:{line}
- **Issue:** {concrete_problem_that_causes_incorrect_behavior}
- **Proof:** {specific_scenario_where_this_fails}
- **Fix:** {exact_code_change_needed}
- **Priority:** HIGH

## ⚡ PERFORMANCE (Provable inefficiencies with O(n) analysis)

### [PERF-001] {descriptive_issue_name}
- **Location:** {file_path}:{line}
- **Current Complexity:** {e.g., O(n) where n is total pool size}
- **Optimal Complexity:** {e.g., O(k) where k is requested count}
- **Impact:** {measurable_impact_in_production_scenario}
- **Fix:** {specific_optimization_with_code}
- **Priority:** HIGH/MEDIUM

## 🔧 REFACTOR (Code organization improvements)

### [REFACTOR-001] {descriptive_issue_name}
- **Location:** {file_path}:{line}
- **Problem:** {maintenance_or_consistency_issue}
- **Benefit:** {concrete_improvement_this_brings}
- **Change:** {specific_refactoring_approach}
- **Priority:** MEDIUM/LOW

## 💡 FEATURE (Missing functionality suggestions)

### [FEATURE-001] {descriptive_issue_name}
- **Location:** {file_path}:{line}
- **Current Gap:** {what_is_missing}
- **Use Case:** {why_this_would_be_valuable}
- **Implementation:** {how_to_add_this_feature}
- **Priority:** MEDIUM/LOW
```

Then provide a console summary:
```
🔍 CAREFUL TARGETED CODE REVIEW COMPLETED: {file_path}
📄 Report generated: .report/{file_path}.md
```

## 🚨🚨🚨 CRITICAL MANDATORY STEP - DO NOT SKIP 🚨🚨🚨
## **MANDATORY DEEP VALIDATION AND CLEANUP STEP:**

### ⚠️ WARNING: FAILURE TO EXECUTE THIS STEP WILL RESULT IN FALSE POSITIVES ⚠️

After completing the initial review and generating the report, you **ABSOLUTELY MUST**:

1. **🔴 STOP AND COUNT**: Count ALL issues found (BUG-xxx, PERF-xxx, REFACTOR-xxx, FEATURE-xxx)
2. **🔴 FOR EVERY SINGLE ISSUE** without exception, IMMEDIATELY execute `/code-review-deep`
3. **🔴 DO NOT PROCEED** to any other task until ALL deep validations are complete
4. **🔴 VALIDATION COMMAND**: `/code-review-deep .report/{file_path}.md {issue_id}`
5. **🔴 REMOVE FALSE POSITIVES**: After ALL validations:
   - Identify ALL issues marked as INVALID-* or QUESTIONABLE
   - COMPLETELY REMOVE these false positive sections from the report
   - Keep ONLY issues marked as VALID-HIGH, VALID-MEDIUM, or VALID-LOW
6. **🔴 UPDATE REPORT**: Clean up the report to show only validated issues

### 📋 **REQUIRED EXECUTION CHECKLIST:**
```
☐ Initial review complete, report written to .report/{file_path}.md
☐ Count total issues found: _____ issues
☐ Execute /code-review-deep for EACH issue:
  ☐ Issue #1: /code-review-deep .report/{file_path}.md {issue_id_1}
  ☐ Issue #2: /code-review-deep .report/{file_path}.md {issue_id_2}
  ☐ Issue #3: /code-review-deep .report/{file_path}.md {issue_id_3}
  ☐ ... (continue for ALL issues)
☐ Read updated report with Deep Analysis sections
☐ Remove ALL INVALID-* issues from report
☐ Remove ALL QUESTIONABLE issues from report
☐ Update report summary with final validated issue count
☐ Final report contains ONLY VALID issues
```

### 🔥 **CRITICAL IMPLEMENTATION STEPS:**
```
STEP 1 - INITIAL REVIEW:
- Complete the code review
- Write initial report with all findings
- STOP HERE - DO NOT CONTINUE WITHOUT VALIDATION

STEP 2 - DEEP VALIDATION (MANDATORY):
- For EACH issue (BUG-001, PERF-001, etc.):
  Execute: /code-review-deep .report/{file_path}.md {issue_id}
- This MUST be done for EVERY SINGLE ISSUE
- NO EXCEPTIONS - even "obvious" issues need validation

STEP 3 - CLEANUP FALSE POSITIVES:
- Read the validated report
- Find ALL issues marked INVALID-* or QUESTIONABLE
- DELETE these entire sections from the report
- Update issue counts in the summary

STEP 4 - FINAL REPORT:
- Contains ONLY VALID-HIGH/MEDIUM/LOW issues
- All false positives removed
- Accurate, validated findings only
```

### ⚡ **CONCRETE EXAMPLE WITH COMMANDS:**
```bash
# Initial review finds 5 issues
# NOW YOU MUST RUN THESE COMMANDS:

/code-review-deep .report/src_main.rs.md BUG-001      # → INVALID-SEMANTICS
/code-review-deep .report/src_main.rs.md BUG-002      # → VALID-HIGH
/code-review-deep .report/src_main.rs.md PERF-001     # → INVALID-ASSUMPTIONS
/code-review-deep .report/src_main.rs.md REFACTOR-001 # → VALID-LOW
/code-review-deep .report/src_main.rs.md REFACTOR-002 # → QUESTIONABLE

# After validation, REMOVE from report:
# - BUG-001 (INVALID)
# - PERF-001 (INVALID)
# - REFACTOR-002 (QUESTIONABLE)

# Final report contains ONLY:
# - BUG-002 (VALID-HIGH)
# - REFACTOR-001 (VALID-LOW)
```

### 🛑 **STOP SIGNS - DO NOT IGNORE:**
- 🛑 If you found issues but haven't run /code-review-deep → STOP and run it
- 🛑 If you're about to finish without validation → STOP and validate
- 🛑 If you're unsure whether to validate → YES, ALWAYS validate
- 🛑 No issue is too small or obvious to skip validation

### ❌ **COMMON MISTAKES TO AVOID:**
- ❌ Forgetting to run /code-review-deep for some issues
- ❌ Assuming an issue is "obviously correct" without validation
- ❌ Leaving INVALID or QUESTIONABLE issues in the final report
- ❌ Not updating the summary after removing false positives
- ❌ Skipping validation because the issue "seems valid"

### ✅ **SUCCESS CRITERIA:**
The review is ONLY complete when:
1. ✅ Every single issue has been validated with /code-review-deep
2. ✅ All INVALID-* issues have been removed from the report
3. ✅ All QUESTIONABLE issues have been removed from the report
4. ✅ The report contains ONLY VALID-HIGH/MEDIUM/LOW issues
5. ✅ The summary accurately reflects the final validated issue count

**HIGH-VALUE ISSUE CRITERIA:**
Issues must meet these standards to be worth reporting:
1. **Algorithm Complexity**: Must show improvement from O(n²) to O(n) or better
2. **Performance Impact**: Must quantify improvement (e.g., "20,000 operations → 1 lookup")
3. **Logic Errors**: Must provide specific input that produces wrong output
4. **Resource Leaks**: Must identify specific path where resources won't be released

**QUANTIFICATION REQUIREMENTS:**
Every reported issue must answer:
- **Impact Scale**: How many users/operations affected?
- **Performance Gain**: Is improvement 2x, 10x, or more?
- **Cost vs Benefit**: Does the fix complexity justify the improvement?
- **Production Impact**: What's the real-world effect in production scenarios?

**CRITICAL REVIEW GUIDELINES:**
- **QUALITY OVER QUANTITY**: Report 3 high-value issues rather than 10 theoretical ones
- **PROVE IT**: Every issue must have concrete evidence or reproducible scenario
- **TRUST DEVELOPERS**: If code has survived in production, it probably works
- **FOCUS ON HOT PATHS**: Performance only matters where code runs frequently
- **RESPECT COMMENTS**: If a comment explains why something is done, believe it
- **ACTUAL vs THEORETICAL**: Only report what WILL fail, not what MIGHT fail
- **SKIP THE OBVIOUS**: Don't report well-known trade-offs or documented decisions

**ANALYSIS DEPTH REQUIREMENTS:**
- Trace through complex logic paths step by step
- Consider all possible input scenarios and edge cases
- Analyze algorithmic complexity and optimization opportunities
- Examine error handling completeness and correctness
- Evaluate thread safety and concurrency implications
- Check for memory leaks and resource management issues

**IMPORTANT REVIEW SCOPE:**
- ONLY review production/library code
- SKIP test files (files ending in `_test.rs`, `test.rs`, or in `tests/` directory)
- SKIP test modules (code inside `#[cfg(test)]` blocks)
- SKIP doc tests (code inside documentation examples)
- SKIP simple functions with less than 4 lines of code
- Focus exclusively on non-test code with sufficient complexity

**STRICT SINGLE FILE FOCUS:**
- ONLY analyze the specified file: {file_path}
- DO NOT continue reviewing other files from any input list
- DO NOT suggest reviewing additional files
- Complete the review and stop after analyzing the single target file

Provide detailed analysis of: {file_path}
