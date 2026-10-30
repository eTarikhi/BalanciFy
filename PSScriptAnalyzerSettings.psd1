@{
    # Keep the default rules active; surface actionable Error and Warning findings.
    IncludeDefaultRules = $true
    Severity = @('Error', 'Warning')

    # No rules are suppressed. Add an exclusion only for a named rule with a
    # documented platform/scope rationale and a narrow, reviewed justification.
    ExcludeRules = @()
}
