# Evidence Record: <claim / acceptance criterion>

- **Evidence ID:** <unique identifier>
- **Project:** <project name / stable ID>
- **Task / Issue / PR:** <canonical link>
- **Recorded at (UTC):** <YYYY-MM-DD HH:MM:SSZ>
- **Evidence type:** LOCAL_TEST | STATIC_CHECK | HOSTED_CI | SIMULATION | REAL_TWO_HOST_LAB | MANUAL_REVIEW
- **Result:** PASS | FAIL | UNKNOWN | NOT RUN

## Provenance

- **Repository:** `<owner>/<repository>`
- **Exact commit SHA tested:** `<full SHA>`
- **Branch / PR:** <branch and PR link>
- **Workflow / run ID:** <URL and numeric ID, or not applicable>
- **Command / procedure:** `<exact command or reviewed procedure>`
- **Environment:** <runner/OS/version or LAB role labels; never host addresses>

## Observation

- **Claim tested:** <one precise statement>
- **Observed output:** <short factual summary; attach/link complete safe log>
- **Expected result:** <acceptance condition>
- **Actual result:** <what happened>
- **Cleanup:** PASS | FAIL | UNKNOWN | NOT APPLICABLE — <observed evidence>

## Scope and limitations

- <What this evidence proves>
- <What it does not prove>
- <Simulation must be labelled simulation; local/CI results do not prove real two-host behavior>

## Safety and redaction

- No secrets, tokens, private keys, credential values, host addresses, or sensitive operational details are included.
- Secret variable names may be named only when necessary; never record their values.
- Redacted log/reference: <safe link or `none`>

## Review

- **Reviewed by:** <QA/Integrator>
- **Disposition:** ACCEPTED | REJECTED | NEEDS_MORE_EVIDENCE | NOT REVIEWED
- **Review note:** <reason and follow-up>
- **Canonical board entry:** <canonical project-board comment/link>
