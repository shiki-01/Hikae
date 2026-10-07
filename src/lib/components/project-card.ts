import type { Project } from '#lib/api/types.js';
import { resolveStatus, type StatusState } from './status-header';

export type CardStatus = StatusState | 'folder_missing';

export function cardStatus(project: Project): CardStatus {
	if (project.folderMissing) return 'folder_missing';
	return resolveStatus(
		project.hasConflict,
		false,
		true,
		project.unsavedCount,
		project.fetchPendingCount,
		project.uploadPendingCount
	);
}

export function needsAttention(project: Project): boolean {
	return project.folderMissing || project.hasConflict;
}
