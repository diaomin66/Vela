import type { EvaluationRecord, EvaluationRun } from './types';

export function recordsForRun(run: EvaluationRun): EvaluationRecord[] {
  return run.results.map((result) => ({
    id: JSON.stringify([run.id, result.profileId, result.modelId, result.caseId]),
    runId: run.id,
    createdAt: run.startedAt,
    trigger: run.trigger,
    caseVersion: run.caseVersion,
    profileId: result.profileId,
    channelName: result.channelName,
    modelId: result.modelId,
    modelAlias: result.modelAlias,
    reasoningEffort: result.reasoningEffort,
    caseId: result.caseId,
    status: result.status,
    score: result.score,
    elapsedMs: result.elapsedMs,
    hasArtifact: Boolean(result.artifactHtml || result.safeSvg),
  }));
}
