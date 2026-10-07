import { createQuery } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import type { RestoreScope } from '#lib/api/types.js';
import { scopeKey } from './restore';

export function useHistory(getProjectId: () => string) {
	return createQuery(() => ({
		queryKey: keys.history(getProjectId()),
		queryFn: () => api.listSavePoints(getProjectId()),
		enabled: getProjectId() !== ''
	}));
}

export function usePointFiles(getProjectId: () => string, getPointId: () => string | null) {
	return createQuery(() => ({
		queryKey: keys.pointFiles(getProjectId(), getPointId() ?? ''),
		queryFn: () => api.listPointFiles(getProjectId(), getPointId() ?? ''),
		enabled: getProjectId() !== '' && getPointId() !== null && getPointId() !== 'now'
	}));
}

export function useFilesAt(
	getProjectId: () => string,
	getPointId: () => string | null,
	getEnabled: () => boolean
) {
	return createQuery(() => ({
		queryKey: keys.filesAt(getProjectId(), getPointId() ?? ''),
		queryFn: () => api.listFilesAt(getProjectId(), getPointId() ?? ''),
		enabled:
			getEnabled() && getProjectId() !== '' && getPointId() !== null && getPointId() !== 'now'
	}));
}

export function useImpact(
	getProjectId: () => string,
	getTargetId: () => string | null,
	getScope: () => RestoreScope
) {
	return createQuery(() => ({
		queryKey: keys.impact(getProjectId(), getTargetId() ?? '', scopeKey(getScope())),
		queryFn: () => api.restoreImpact(getProjectId(), getTargetId() ?? '', getScope()),
		enabled: getProjectId() !== '' && getTargetId() !== null,
		gcTime: 0
	}));
}
