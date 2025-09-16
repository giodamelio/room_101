# /code-review-deep

**Description:** Deep validation analysis of specific issues from Rust code review reports using advanced reasoning

## Parameters
- `report_file_path` (string, required): Path to the review report file to analyze
- `issue_id` (string, required): Specific issue ID to validate (e.g., "BUG-001", "PERF-002", "CRITICAL-001")

## Prompt

You are performing an ULTRA-DEEP VALIDATION of a specific issue identified in a Rust code review report. Your goal is to determine whether the reported issue is genuinely problematic or a false positive.

**ULTRA-DEEP THINKING PROCESS:**

## Phase 1: EXTRACT AND UNDERSTAND THE ISSUE
1. **Read the report file** at {report_file_path}
2. **Extract the specific issue** identified by {issue_id}
3. **Understand the claimed problem**: What does the report say is wrong?
4. **Identify the exact code location** mentioned in the issue
5. **Extract the proposed fix** if provided

## Phase 2: DEEP SOURCE CODE ANALYSIS
1. **Read the actual source file** mentioned in the issue location
2. **Understand the complete context** around the problematic code
3. **Trace through the code logic** step by step
4. **Analyze the data flow** and dependencies
5. **Study the broader codebase architecture** that interacts with this code
6. **Check for existing tests** that validate the behavior
7. **Look for comments or documentation** explaining the design decisions

## Phase 3: CRITICAL VALIDATION ANALYSIS
Use your deepest reasoning capabilities to analyze:

### A. TECHNICAL CORRECTNESS
- **Is the technical claim accurate?** Does the code actually have the reported problem?
- **Are the assumptions correct?** Does the reporter understand the code correctly?
- **What are the real semantics?** How does this code actually behave?
- **Are there protective mechanisms?** Does existing code already handle the concern?

### B. REAL-WORLD IMPACT ANALYSIS
- **Can this actually happen?** Provide concrete scenarios where this would occur
- **What are the prerequisites?** What conditions must be met for this to be a problem?
- **How likely is it?** In production usage, would this realistically manifest?
- **What's the actual impact?** If it did happen, what would be the consequence?

### C. DESIGN PATTERN VALIDATION
- **Is this a known pattern?** Is this a standard Rust/async pattern that's safe?
- **Does the API design expect this?** Are the traits/types designed for this usage?
- **Are there precedents?** How is similar code handled elsewhere in the codebase?
- **Is there existing validation?** Do tests or other code prove this works?

### D. COST-BENEFIT ANALYSIS
- **Fix complexity**: How difficult would the proposed fix be?
- **Risk of fix**: Could the proposed fix introduce new problems?
- **Performance impact**: Would the fix affect performance?
- **Maintenance burden**: Would the fix make the code harder to maintain?

## Phase 4: EVIDENCE GATHERING
For each aspect of your analysis, provide concrete evidence:
- **Code snippets** that prove your point
- **Type system guarantees** that prevent the issue
- **Test cases** that demonstrate correct behavior
- **Documentation** that explains the design
- **Similar patterns** in the codebase that work correctly

## Phase 5: FINAL VALIDATION VERDICT

Based on your ultra-deep analysis, classify the issue as:

- **VALID-HIGH**: Real issue with significant impact, fix recommended
- **VALID-MEDIUM**: Real issue with moderate impact, fix optional
- **VALID-LOW**: Real issue with minimal impact, fix optional
- **QUESTIONABLE**: Issue may exist but impact/likelihood is unclear
- **INVALID-ASSUMPTIONS**: Issue based on incorrect understanding of the code
- **INVALID-SEMANTICS**: Issue based on misunderstanding of Rust/async semantics
- **INVALID-DESIGN**: Issue contradicts the intended design pattern
- **INVALID-PRECEDENT**: Issue contradicts established patterns in the codebase

## VALIDATION OUTPUT FORMAT

After completing your analysis, update the original report by adding a **Deep Analysis** section to the specific issue:

```markdown
- **Deep Analysis Status**: {VALID-HIGH/VALID-MEDIUM/VALID-LOW/QUESTIONABLE/INVALID-*}
- **Validation Summary**: {2-3 sentence summary of your findings}
- **Technical Analysis**: {Detailed explanation of why this is/isn't a real issue}
- **Real-world Scenarios**: {Concrete scenarios where this would/wouldn't manifest}
- **Evidence**: {Code snippets, tests, or patterns that support your conclusion}
- **Recommendation**: {Whether to fix, ignore, or investigate further}
```

## CRITICAL VALIDATION GUIDELINES

**ASSUME THE DEVELOPERS ARE COMPETENT**:
- If code has been in production, assume it works unless proven otherwise
- If patterns are consistent across the codebase, assume they're intentional
- If tests exist and pass, assume the behavior is correct
- If comments explain the logic, believe them unless contradicted by evidence

**PROVE YOUR CLAIMS**:
- Every VALID classification must include concrete failure scenarios
- Every INVALID classification must include technical proof
- Provide specific code examples that demonstrate your point
- Quote documentation or tests that support your analysis

**FOCUS ON REAL-WORLD IMPACT**:
- Theoretical issues without practical manifestation are usually INVALID
- Performance issues must show measurable impact in realistic scenarios
- Race conditions must show actual problematic interleavings
- Resource leaks must show unbounded growth in practice

**UNDERSTAND RUST SEMANTICS DEEPLY**:
- Arc::clone() vs deep copying
- Send/Sync implications for thread safety
- Ownership and borrowing guarantees
- Async/await execution models
- Type system guarantees

## EXAMPLE VALIDATION

For reference, here's how you validated BUG-001 from the batcher.rs report:

**Issue Claim**: "Race condition where pool.clone() might be stale"
**Validation Result**: INVALID-SEMANTICS
**Reasoning**: Pool::clone() is Arc::clone(), not deep copy. All clones share same underlying state. No mechanism for pool replacement exists in the API. This is standard Rust shared ownership pattern.
**Evidence**: Pool struct contains Arc<PoolInner>, Clone implementation uses Arc::clone, no mutation APIs exist.

Now perform the same ultra-deep validation for issue {issue_id} in report {report_file_path}.

**IMPLEMENTATION STEPS:**

1. **Read the report file** using the Read tool
2. **Extract the specific issue** by searching for the {issue_id} section
3. **Analyze the source code** mentioned in the issue location
4. **Perform your ultra-deep validation analysis**
5. **Update the report file** using the Edit tool to add the Deep Analysis section

**REQUIRED ACTIONS:**
- Use Read tool to load {report_file_path}
- Use Read tool to examine the source code file mentioned in the issue
- Use Edit tool to add the Deep Analysis section to the specific issue in the report
- The Deep Analysis section should be inserted right after the existing issue content, before the next issue or section

**FILE UPDATE FORMAT:**
Find the issue section (e.g., `### [BUG-001] Issue Name`) and add the Deep Analysis immediately after the existing content:

```markdown
### [BUG-001] Issue Name
- **Location:** file:line
- **Issue:** existing issue description
- **Fix:** existing fix description
- **Priority:** existing priority
- **Deep Analysis Status**: {VALID-HIGH/VALID-MEDIUM/VALID-LOW/QUESTIONABLE/INVALID-*}
- **Validation Summary**: {2-3 sentence summary of your findings}
- **Technical Analysis**: {Detailed explanation of why this is/isn't a real issue}
- **Real-world Scenarios**: {Concrete scenarios where this would/wouldn't manifest}
- **Evidence**: {Code snippets, tests, or patterns that support your conclusion}
- **Recommendation**: {Whether to fix, ignore, or investigate further}
- **Human Validation Required**: {true/false - whether human review is still needed}
```

**REMEMBER**:
- You MUST actually update the report file, not just describe the analysis
- Use your most advanced reasoning capabilities for the validation
- This is a deep investigation into whether the claimed issue has merit
- Be thorough, be precise, and be honest about what you find
- Always use the Edit tool to modify the original report file with your findings
