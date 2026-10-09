import type { Project } from '#lib/api/types.js';
import { resolveStatus, type StatusState } from './status-header';

export type CardStatus = StatusState | 'folder_missing' | 'repo_broken';

export function cardStatus(project: Project): CardStatus {
	if (project.folderMissing) return 'folder_missing';
	if (project.repoBroken) return 'repo_broken';
	return resolveStatus(
		project.hasConflict,
		false,
		true,
		project.unsavedCount,
		project.fetchPendingCount,
		project.uploadPendingCount,
		false,
		project.interruptedOperation !== null
	);
}

export function needsAttention(project: Project): boolean {
	return (
		project.folderMissing ||
		project.repoBroken ||
		project.hasConflict ||
		project.interruptedOperation !== null
	);
}

/** GitHub に接続していないプロジェクトか（フォルダが見つからないものは、先にフォルダの対応が必要なので除く） */
export function needsConnection(project: Project): boolean {
	return !project.remoteConnected && !project.folderMissing && !project.repoBroken;
}
