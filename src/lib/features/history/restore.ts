import type { RestoreScope } from '#lib/api/types.js';

export function scopeKey(scope: RestoreScope): string {
	return scope.kind === 'all' ? 'all' : `file:${scope.path}`;
}
