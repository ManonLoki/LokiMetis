/** 看板界面英文资源，键结构与中文资源对齐。 */
import { dashboardUsageSettingsEnUS } from "./dashboardUsageSettings";

export const dashboardEnUS = {
  app: {
    brand: "LokiMetis",
    title: "LokiMetis",
    version: "{{version}}",
  },
  bootstrap: {
    loading: "Reading initialization status…",
    errorTitle: "Unable to read initialization status",
    errorBody: "The dashboard is staying closed until setup is complete.",
    retry: "Retry",
  },
  language: {
    label: "Display language",
    system: "Use system language",
    systemResolved: "Use system language ({{language}})",
    zhCN: "简体中文",
    enUS: "English",
    saving: "Saving language preference…",
    saveError: "Unable to save the language preference. Try again.",
  },
  shell: {
    clientAll: "All",
    clientSubtitle: {
      codex: "Local rollouts · Separate statistics",
      claudeCode: "Local Claude Code transcripts · Separate statistics",
      grokBuildCli: "Local Grok sessions · Separate statistics",
    },
    clientSelectorAria: "Agent client currently being viewed",
    localScanProgress: {
      title: "{{client}} is computing local statistics",
      aria: "{{client}} local statistics progress",
    },
    noEnabledAgents: {
      title: "No AI agents are enabled",
      body: "Enable the agents you want to use under Agent configuration in Settings. Disabled agents stay out of the header and all local scanning, statistics, and monitoring.",
    },
    navigation: {
      aria: "Main navigation",
      pagesAria: "Dashboard pages",
      chartsPagesAria: "Chart pages",
      dashboardSection: "Dashboard",
      dashboardDescription: "View local usage and data sources",
      chartsSection: "Charts",
      chartsDescription: "View overview trends and multidimensional usage charts",
      settingsSection: "Settings",
      itemAria: "{{label}}: {{description}}",
      overview: {
        label: "Overview",
        description: "View local records and fixed calendar-day windows",
      },
      chartOverview: {
        label: "Overview",
        description: "View Token and call trend charts",
      },
      chartUsage: {
        label: "Usage",
        description: "View usage distributions by fixed dimensions",
      },
      statistics: {
        label: "Usage",
        description: "View local trends and groups by fixed dimensions",
      },
      charts: {
        label: "Charts",
        description: "View Token trends and multidimensional usage distributions",
      },
      calls: {
        label: "Calls",
        description: "Browse local calls and their complete Token breakdown, page by page",
      },
      sources: {
        label: "Data sources",
        description: "View local data directories and scan coverage",
      },
      settings: {
        label: "Settings",
        description: "Manage scan cadence and automatic cleanup",
      },
      workbuddy: {
        label: "WorkBuddy",
        description:
          "View WorkBuddy project JSONL requests, models, and token/credit trends",
      },
      privacy: {
        label: "Privacy & settings",
        description: "Manage language, scan cadence, and this app’s local index",
      },
    },
  },
  ui: {
    tokenExact: "Exact value: {{value}} tokens",
    tokenExactCopy: "Copy exact value",
    tokenExactCopied: "Copied",
    implementation: {
      badge: "Feature in progress",
      title: "Usage data is not connected yet",
      fallback:
        "The real provider and local index are being integrated. No collected data is displayed or fabricated.",
    },
    loadingDefault: "Loading usage data",
    loadingVisible: "{{label}}…",
    failureTitle: "Unable to load data right now",
    localIndex: {
      needsRescan: {
        title: "Local index needs to be rescanned",
        body: "This local index was created by an older parser. Old data will not be presented as current statistics. Run an explicit quick scan to rebuild it safely.",
        action: "Go to Data Sources to rescan",
      },
      notScanned: {
        title: "Local usage index has not been created",
        body: "Local records for the selected client have not been scanned, so zeros cannot be treated as confirmed zero usage. Scans read only authorized data directories.",
        action: "Go to Data Sources for a quick scan",
      },
      readyNoCalls: {
        title: "No usable calls in the scanned scope",
        body: "A scan has completed, but the current parser found no calls that can be included in statistics. Check the coverage report or run another quick scan.",
        action: "Go to Data Sources to rescan",
      },
    },
    scanProgress: "Scan progress",
  },
  router: {
    error: {
      title: "Page failed to load",
      reload: "Reload page",
    },
    notFound: {
      title: "Page not found",
      body: "This address is not an approved app page. Use the navigation to return.",
      returnOverview: "Return to Overview",
    },
  },
  window: {
    billingPeriod: "Current billing period",
    today: "Today",
    yesterday: "Yesterday",
    thisWeek: "This week",
    lastWeek: "Last week",
    thisMonth: "This month",
    lastMonth: "Last month",
  },
  dimension: {
    group: "Group",
    agent: "AI agent",
    model: "Model",
    reasoningEffort: "Reasoning effort",
    project: "Project",
    thread: "Thread",
    root: "Data directory",
  },
  metric: {
    totalTokens: "Total tokens",
    input: "Input",
    cachedInput: "Cached input",
    cacheWrite: "Cache write",
    uncachedInput: "Uncached input",
    output: "Output",
    reasoningOutput: "Reasoning output",
    calls: "Calls",
    cacheReadShare: "Cache-read share",
  },
  overview: {
    errors: {
      missingSections: "The usage overview is missing this client’s local-record section.",
      incompleteWindows: "The local statistics windows are incomplete. Try again.",
    },
    partialTitle: "Some data is temporarily unavailable",
    scopeDivider: "Separate scopes",
    local: {
      title: "Local records",
      description: "Covers only authorized, indexed, and deduplicated data directories.",
      windowAria: "Local statistics window",
    },
  },
  overviewCards: {
    local: {
      sectionAria: "{{window}} local-record window",
      callCount: "Total calls",
      callDedupe: "Deduplicated by stable call ID",
      tokenTotal: "Total tokens",
      totalDerived: "Total derived from input + output",
      totalUpstream: "Summed from per-call upstream totals",
      breakdownTitle: "{{window}} token breakdown",
      breakdownDescription:
        "Cached input is a subset of input, and reasoning output is an analytical view of output. Neither is added to the total twice.",
      coverageTitle: "Cache and data coverage",
      enabledRootsBadge: "Enabled-root aggregate",
      cachedReadCalls: "Calls with cache reads",
      threads: "Threads",
      roots: "Aggregated data directories",
      sources: "Source files",
      duplicates: "Cross-root duplicate observations",
    },
  },
  statistics: {
    page: {
      title: "Usage statistics",
      codexDescription:
        "View trends by the saved time standard and reconcilable groups by model, reasoning effort, project, thread, or data directory.",
      clientDescription:
        "View {{client}} trends by the saved time standard and separate, reconcilable groups by model, project, thread, or data directory.",
      loading: "Loading local usage statistics",
    },
    controls: {
      aria: "Local usage statistics controls",
      window: "Statistics window",
      dimension: "Group by",
      updating: "Updating",
      sameSnapshot: "Same snapshot",
      localRecords: "Local records",
      observed: "Observed {{date}} · Refreshes 10 seconds after each read completes",
    },
    summary: {
      totalTokens: "Total tokens",
      calls: "Deduplicated calls",
      threads: "Deduplicated threads",
      cacheReadShare: "Cache-read share",
    },
    methodology: {
      title: "Methodology",
      body: "Date buckets, groups, and totals come from the same indexed, deduplicated snapshot; this query does not scan disk. Copies across data directories are assigned to one deterministic data directory, while complete source coverage remains visible under Data Sources.",
    },
    daily: {
      title: "Daily trend",
      descriptionLocal:
        "Uses this device’s time-zone calendar days; zero-value dates are not omitted.",
      descriptionRemote: "Uses UTC calendar days; zero-value dates are not omitted.",
      dateLocal: "Local date",
      dateRemote: "UTC date",
      dateCustom: "UTC date",
      inProgress: "In progress",
    },
    groups: {
      title: "Grouped statistics",
      description:
        "Uses the same time-standard window as the daily trend. Shows up to 10 groups; additional groups are combined as Other and still reconcile to the total.",
      merged: "Combined",
      tokenShare: "Share of total",
      quality: "Quality",
      empty: "There are no local calls to group in this window.",
    },
  },
  charts: {
    loading: "Loading local charts",
    controls: {
      window: "Time range",
      dimension: "Group by",
      metric: "Chart metric",
    },
    overview: {
      tokensTitle: "Token trends",
      tokensDescription:
        "Select several Token metrics at once. Unavailable components break the line instead of being drawn as zero.",
      metricSelectorAria: "Token trend metrics",
      tokensChartAria: "Token trends across complete time buckets",
      callsTitle: "Call trend",
      callsChartAria: "Call-count trend on a separate vertical axis",
    },
    distribution: {
      title: "Multidimensional usage distribution",
      description:
        "Shows Top 10 groups plus a reconciling remainder across fixed dimensions. Switch between Token components and call count.",
      barAria: "Usage distribution bar chart",
      totalTokenShare: "Share of total tokens: {{share}}",
      empty: "There are no local calls to group in this window.",
    },
  },
  calls: {
    page: {
      title: "Calls",
      description:
        "Browse all filtered and deduplicated calls with a stable cursor. Message content is not read, and absolute paths are never sent to the interface.",
      loading: "Loading local calls",
      missingFirstPage: "The calls query did not return its first page.",
    },
    filters: {
      title: "Call filters",
      description:
        "Options come from all enabled roots. Changing a filter or sort starts again from the first cursor.",
      all: "All",
      clear: "Clear filters",
    },
    refresh: {
      title: "Unable to refresh call results",
      retained:
        "The page structure is preserved and old results are hidden. Retry the current filters and sort.",
      retry: "Retry refresh",
    },
    table: {
      aria: "Call results table; scroll horizontally for more columns",
      agent: "Agent",
      time: "Time",
      inputTokens: "Input tokens",
      updating: "Updating filtered and sorted results…",
      replacementFailed:
        "Unable to read results for the current filters and sort. Try again.",
      emptyScanned: "No usable local calls were found in the scanned scope.",
      emptyFiltered: "No local calls match the current filters.",
    },
    footer: {
      readingFirstPage: "Reading the new first page",
      firstPageFailed: "Unable to read the new first page",
      loaded: "{{loaded}} / {{total}} calls loaded",
      controlsAvailable: "Filter and sort controls remain available",
      retryAbove: "Retry above",
      loadMore: "Load more",
      allLoaded: "All calls have been loaded",
    },
    nextPage: {
      title: "Unable to load the next page",
      retained: "Previously loaded calls remain visible. Retry the current next page.",
      retry: "Retry next page",
    },
  },
  sources: {
    page: {
      title: "Data sources",
      description:
        "Check local data directories and scan coverage. Hits for the current tab are deduplicated and added to the registered list directly.",
      addRoot: "Add data directory",
      loading: "Loading data-source status",
      updatedTitle: "Data directory updated",
      updateErrorTitle: "Unable to update the data-root index",
      updateErrorBody:
        "The data-root update failed. Original {{client}} files were not modified.",
      scanErrorTitle: "Unable to run the scan",
      scanErrorBody:
        "The scan operation failed. Original {{client}} files were not modified.",
    },
    background: {
      runningTitle: "Background Token statistics running",
      runningBody:
        "Updating local {{client}} usage. The page refreshes automatically when done.",
      failedTitle: "Background Token statistics did not finish",
      failedBody: "{{client}} will retry during the next scheduled cycle.",
    },
    empty: {
      title: "No local {{client}} data directory found",
      codexBody:
        "Quick scan only checks this platform’s priority relative directories and their contents. Scan hits are deduplicated into the registered list.",
      claudeBody:
        "Quick scan only checks this platform’s priority relative directories and their contents. Scan hits are deduplicated into the registered list.",
      grokBody:
        "Quick scan only checks GROK_HOME or ~/.grok and sessions/**/updates.jsonl inside them. Scan hits are deduplicated into the registered list.",
      action: "Discover data directories",
      fullDeviceAction: "Discover data directories",
      addRootAction: "Add candidate roots",
    },
    primary: {
      title: "Codex primary data directory",
      badge: "Local primary directory",
      description:
        "An enabled Codex data directory that passes re-verification can be the only primary directory. This app does not read auth.json.",
      label: "Codex primary data directory",
      duplicate: "{{alias}} (ID {{id}})",
      empty: "No enabled data directories",
      placeholder: "Use this process’s Codex context",
      clear: "Clear primary directory selection",
    },
    scan: {
      title: "Scan controls",
      state: {
        cancelled: "Cancelled",
        completed: "Completed",
        failed: "Failed",
        idle: "Idle",
        running: "Scanning",
      },
      quick: "Quick scan",
      fullDevice: "Full-device discovery",
      updateIndex: "Background statistics",
      cancel: "Cancel scan",
      currentScope: "Current scope: {{scope}}",
      filesVisited: "Files visited: {{count}}",
      callsIndexed: "Calls indexed: {{count}}",
    },
    discovery: {
      title: "Data source discovery",
      description:
        "Checks local volumes, directory names, and file names only. Hits for the current tab are registered directly; JSONL is not opened first.",
      startUser: "Quick scan",
      startFull: "Full-disk scan",
      cancel: "Cancel scan",
      add: "Add",
      volumes: "Volumes completed: {{completed}} / {{total}}",
      directories: "Directories checked: {{count}}",
      fileNames: "File names checked: {{count}}",
      candidates: "Candidates: {{count}}",
      partialTitle: "Discovery coverage is partial",
      partialBody:
        "Permission denied: {{denied}}; I/O errors: {{errors}}; policy skips: {{skipped}}.",
      skippedBody: "I/O errors: {{errors}}; link or policy skips: {{skipped}}.",
      failedTitle: "Discovery failed",
      failedBody:
        "Stable error code: {{code}}. Fix the environment and start discovery again.",
      strategy: {
        windowsSearch: "Windows Search",
        macOsSpotlight: "Spotlight",
        metadataTraversal: "File-name traversal",
      },
      platform: {
        other:
          "This platform uses data source discovery across confirmed local directories.",
      },
      scope: {
        userPriority: "Current scope: platform priority directories",
        fullLocalVolumes: "Current scope: all confirmed local volumes",
        manualSubtree: "Current scope: manually selected folder and its subtree",
      },
      addManualClient: "Add {{client}} data directory",
      evidence: {
        codexRollout: "Codex rollout",
        claudeTranscript: "Claude transcript",
        claudeSubagent: "Claude subagent",
        grokSessionUpdates: "Grok updates.jsonl",
      },
      state: {
        idle: "Idle",
        running: "Running",
        complete: "Scan complete",
        partial: "Partial",
        cancelled: "Cancelled",
        failed: "Failed",
      },
    },
    dialog: {
      renameTitle: "Rename data directory",
      renameLabel: "New data-root alias",
      removeTitle: "Remove data-root index",
      removeBody:
        "Only this app’s index for “{{alias}}” will be removed. Original {{client}} files will not be deleted.",
      removeConfirm: "Remove index",
    },
    table: {
      alias: "Data-root alias",
      discovery: "Discovery method",
      files: "Files",
      skipped: "Skipped",
      errors: "Errors",
      duplicates: "Duplicate sources",
      lastScan: "Last scan",
      actions: "Actions",
      enabled: "Enabled",
      disabled: "Disabled",
      primary: "Primary data directory",
      activation: {
        confirmedUnindexed: "Waiting for background statistics",
        indexing: "Computing statistics",
        validationFailed: "Candidate validation failed",
      },
      enable: "Enable",
      disable: "Disable",
      reindex: "Reindex",
      rename: "Rename",
      remove: "Remove index",
      toggleAria: "{{action}} {{alias}}",
      reindexAria: "Reindex {{alias}}",
      renameAria: "Rename {{alias}}",
      removeAria: "Remove the index for {{alias}}",
      empty: "No data directories are registered.",
    },
  },
  privacy: {
    page: dashboardUsageSettingsEnUS.page,
    language: {
      title: "Display language",
      badge: "Global setting",
    },
    autostart: {
      title: "Start at login",
      badge: "System setting",
      description:
        "Starts this app in the background after you sign in and keeps the tray recovery entry available. The operating-system login item is authoritative; providing this switch does not turn it on for you.",
      loading: "Reading the operating-system login item…",
      pending: "Updating the operating-system login item…",
      enabled: "Enabled",
      disabled: "Disabled",
      errorTitle: "Unable to update start at login",
      errorBody:
        "The operation did not complete. The interface reread and now shows the operating system’s actual state.",
      unknownTitle: "Current state is unknown",
      unknownBody:
        "The operating-system login item cannot be read right now. The switch stays disabled to prevent an incorrect change.",
      retry: "Read actual state again",
    },
    retentionDays: dashboardUsageSettingsEnUS.retentionDays,
    scanInterval: dashboardUsageSettingsEnUS.scanInterval,
  },
  workbuddy: {
    title: "WorkBuddy local usage statistics",
    loading: "Loading WorkBuddy local statistics…",
    lockedTitle: "WorkBuddy local statistics is not enabled",
    lockedDescription:
      "Turn on WorkBuddy local statistics in Settings to see project JSONL requests, sessions, models, and token/credit trends here.",
    configureAction: "Go to Settings",
    totalSessions: "Total sessions",
    totalRequests: "Total requests",
    topLevelRequests: "Top-level requests",
    subagentRequests: "Subagent requests",
    totalTokens: "Total tokens",
    totalCredits: "Total credits",
    averageDuration: "Average session duration",
    minutesValue: "{{minutes}} min",
    millisecondsValue: "{{ms}} ms",
    traceTotal: "Total traces",
    traceErrorRate: "Trace error rate",
    traceAverageDuration: "Average trace duration",
    noDailyData: "No daily usage records yet",
    dailySessionsTitle: "Daily session count",
    dailyRequestsTitle: "Daily request count",
    dailyTokensTitle: "Daily token usage trend",
    dailyCreditsTitle: "Daily credit usage trend",
    hourlySessionsTitle: "Hourly session count",
    hourlyRequestsTitle: "Hourly request count",
    hourlyTokensTitle: "Hourly token usage trend",
    hourlyCreditsTitle: "Hourly credit usage trend",
    breakdownTitle: "{{window}} request usage and Trace diagnostics",
    traceCancelled: "Cancelled traces",
    traceCompleted: "Completed traces",
    traceError: "Error traces",
    summarySessions: "Sessions",
    chartDistributionDescription:
      "Distribute the same project JSONL usage events by calendar day, or show independent Trace status. Cached input is a subset of all input and is never added to the total again.",
    chartShare: "Share of window: {{share}}",
    chartGroup: {
      day: "Calendar day",
      traceStatus: "Trace status",
    },
    chartMetric: {
      tokens: "Tokens",
      inputTokens: "Input tokens",
      cachedInputTokens: "Cached input",
      uncachedInputTokens: "Uncached input",
      outputTokens: "Output tokens",
      requests: "Requests",
      sessions: "Sessions",
      credits: "Credits",
      traceCount: "Trace count",
    },
    modelUsage: {
      title: "Actual model usage details",
      jsonlScope: "Project JSONL scope",
      description:
        "Groups actual execution models from providerData.model on each WorkBuddy project JSONL usage event, including top-level and subagent requests. All input includes cached input, cached input is not added to totals again, and this table reconciles with the statistics above.",
      model: "Actual model",
      requests: "Requests",
      topLevelCalls: "Top-level requests",
      subagentCalls: "Subagent requests",
      unattributed: "Unattributed",
      empty: "No valid project JSONL model usage exists in this window.",
    },
  },
  workbuddySources: {
    title: "WorkBuddy data source",
    description:
      "Read-only detection and parsing of ~/.workbuddy/projects/<project>/<session>.jsonl and its subagents/*.jsonl. No data root or product index is created. JSONL line bytes are read during parsing, but conversation bodies are never retained, displayed, indexed, uploaded, or logged.",
    found: "Found: {{alias}}",
    notFound: "Not found",
    notFoundTitle: "No local WorkBuddy directory found",
    notFoundBody: "Confirm WorkBuddy is installed and has run at least once, then rescan.",
    rescan: "Rescan",
    toggleAria: "Turn WorkBuddy local statistics reading on or off",
    loading: "Detecting the local WorkBuddy directory",
  },
  common: {
    cancel: "Cancel",
    close: "Close",
    confirm: "Confirm",
    loading: "Loading…",
    retry: "Retry",
    save: "Save",
    unknownError: "The operation could not be completed. Try again.",
    notProvided: "Not provided",
    noRecordsYet: "No records yet",
    notApplicable: "Not applicable",
    unknown: "Unknown",
  },
  format: {
    provider: {
      claudeTranscriptJsonl: "Claude Code local transcript",
      combinedLocalAgents: "All enabled agents’ local records",
      grokSessionJsonl: "Grok local session",
      rolloutJsonl: "Codex local rollout",
      workbuddyProjectJsonl: "WorkBuddy project JSONL",
    },
    freshness: {
      expired: "Expired",
      fresh: "Fresh",
      stale: "Refresh due",
      unknown: "Freshness unknown",
    },
    completeness: {
      complete: "Complete coverage",
      partial: "Partial coverage",
      unknown: "Coverage unknown",
    },
    confidence: {
      derived: "Controlled estimate",
      exact: "Direct fact",
      suspected: "Review needed",
    },
  },
  backend: {
    message: {
      overviewLocalIndexUnavailable:
        "The local index is temporarily unavailable. Try again.",
      overviewWorkbuddyUnavailable:
        "WorkBuddy usage is temporarily unavailable. All currently shows only the other enabled agents and is marked as partial coverage. Try again.",
      scanIdle: "No scan has started yet.",
      scanRunning: "Scanning the authorized scope.",
      scanCancelling: "Cancelling the scan. Discovered candidates will be kept.",
      scanCancelled: "The scan was cancelled and coverage is incomplete.",
      scanCompleted: "Scan complete.",
      scanFailed: "The scan failed. Original client files were not modified.",
      sourceAddCancelled: "Adding a data directory was cancelled.",
      sourceRegistered:
        "The data directory was registered. Run a quick scan to build its index.",
      sourceAlreadyRegistered:
        "This directory is already a data source. Its alias, enabled state, and primary selection were preserved.",
      sourceManualDeepSearchStarted:
        "The selected folder is not a valid data root; a deep search under it has started.",
      sourceManualDeepSearchEmpty:
        "No data roots matching the current client signature were found under the selected folder.",
      sourceEnabled:
        "The data directory is enabled and will be included in the next quick scan.",
      sourceDisabled:
        "The data directory is disabled. Its current index is kept and original files were not modified.",
      sourceRenamed: "The data-root alias was updated.",
      sourceRemoved:
        "The data directory was removed from this app’s index. Original files were not modified.",
      primaryChanged: "The Codex primary data directory changed.",
      primaryAlreadySelected: "This directory is already the Codex primary data directory.",
      primaryCleared: "The primary data directory selection was cleared.",
      primaryNotSet: "No Codex primary data directory is selected.",
      indexCleared:
        "This app’s index was cleared. Original client files were not modified.",
    },
    display: {
      unknownModel: "Unknown model",
      unknownReasoningEffort: "Unknown reasoning effort",
      reasoningNone: "None",
      reasoningMinimal: "Minimal",
      reasoningLow: "Low",
      reasoningMedium: "Medium",
      reasoningHigh: "High",
      reasoningXHigh: "Extra high",
      uncategorizedProject: "Uncategorized project",
      project: "Project {{value}}",
      unknownThread: "Unknown thread",
      thread: "Thread {{value}}",
      unnamedRoot: "Unnamed data directory",
      remainder: "Other",
      disambiguated: "{{label}} ({{index}})",
    },
    discovery: {
      defaultRoot: "Default data directory",
      codexEnvironment: "Current CODEX_HOME",
      claudeEnvironment: "Current CLAUDE_CONFIG_DIR",
      userRegistered: "Registered by user",
      fullDevice: "Full-device discovery",
      metadataDiscovery: "Data source discovery",
    },
    scanScope: {
      registeredRoots: "Registered data directories",
      localFixedVolumes: "Local fixed volumes",
      discoveringVolumes:
        "Discovering local volumes: {{directories}} directories checked, {{roots}} data directories found",
      discoveryFinished:
        "Discovery complete: {{directories}} directories checked, {{roots}} data directories found",
      indexingRoots: "{{completed}} / {{total}} data directories complete",
    },
    indexLocation: {
      codex: "This app’s data directory / usage-index.sqlite3 (Codex)",
      claudeCode: "This app’s data directory / clients / claude-code / usage-index.sqlite3",
      grokBuildCli:
        "This app’s data directory / clients / grok-build-cli / usage-index.sqlite3",
    },
  },
};
