/** 构造 Git 多远端发布的目标摘要与部分结果消息。 */

/** 以不含地址或凭据的远端名和分支名描述发布目标。 */
export function describeTargets(targets) {
  if (targets.length === 0) return "none";
  return targets
    .map((target) => `remote '${target.remote}' branch '${target.branch}'`)
    .join(", ");
}

/** 描述主目标结果以及尚未尝试的全部补充目标。 */
export function primaryFailureMessage(
  remote,
  branch,
  detail,
  outcome,
  additionalTargets,
  confirmedAdditional = [],
) {
  if (confirmedAdditional.length > 0) {
    return `Primary Git remote '${remote}' branch '${branch}' ${detail}; current target ${outcome}, ` +
      `previously confirmed additional targets remain recorded (targets: ${describeTargets(confirmedAdditional)}), ` +
      `remaining additional targets were not attempted (targets: ${describeTargets(additionalTargets)}), and the ` +
      "same target arguments can be retried.";
  }
  return `Primary Git remote '${remote}' branch '${branch}' ${detail}; current target ${outcome}, ` +
    `all additional targets were not attempted (targets: ${describeTargets(additionalTargets)}), and the same target ` +
    "arguments can be retried.";
}

/** 描述当前补充目标，以及前后已确认或未尝试的精确范围。 */
export function additionalFailureMessage(
  target,
  detail,
  outcome,
  published,
  remaining,
  confirmedLater = [],
) {
  const laterClause = confirmedLater.length > 0
    ? `later confirmed additional targets remain recorded (targets: ${describeTargets(confirmedLater)}), `
    : "";
  return `Additional Git remote '${target.remote}' branch '${target.branch}' ${detail}; the ` +
    "primary target and earlier additional targets are confirmed published " +
    `(earlier additional targets: ${describeTargets(published)}), current target ${outcome}, ${laterClause}subsequent ` +
    `additional targets were not attempted (targets: ${describeTargets(remaining)}), and the same target arguments can ` +
    "be retried.";
}

/** 按冻结目标位置返回稳定错误码与完整部分结果消息。 */
export function pendingFailure(pending, index, kind, detail, outcome) {
  const targets = pending.targets;
  const target = targets[index];
  const publicTargets = targets.map(({ remote, branch }) => ({ remote, branch }));
  if (targets.length === 1) {
    const stableCodes = {
      "push-failed": "push-rejected",
      "verification-failed": "remote-verification-failed",
      "local-state-changed": "local-state-changed",
      "state-write-failed": "state-write-failed",
    };
    const stableMessages = {
      "push-failed": "Git default branch push could not be confirmed.",
      "verification-failed": "Git remote default branch could not be confirmed at the frozen published HEAD.",
      "local-state-changed": "Git default branch was confirmed published but local Git state changed.",
      "state-write-failed": "Git default branch was confirmed published but lifecycle state could not be saved.",
    };
    return [stableCodes[kind], stableMessages[kind]];
  }
  if (index === 0) {
    const confirmed = publicTargets.slice(1).filter((_item, offset) => targets[offset + 1].confirmed);
    const remaining = publicTargets.slice(1).filter((_item, offset) => !targets[offset + 1].confirmed);
    return [
      `primary-${kind}`,
      primaryFailureMessage(target.remote, target.branch, detail, outcome, remaining, confirmed),
    ];
  }
  const confirmedLater = publicTargets
    .slice(index + 1)
    .filter((_item, offset) => targets[index + 1 + offset].confirmed);
  const remaining = publicTargets
    .slice(index + 1)
    .filter((_item, offset) => !targets[index + 1 + offset].confirmed);
  return [
    `additional-${kind}`,
    additionalFailureMessage(
      publicTargets[index],
      detail,
      outcome,
      publicTargets.slice(1, index),
      remaining,
      confirmedLater,
    ),
  ];
}
