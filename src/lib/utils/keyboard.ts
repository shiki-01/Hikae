export type ArrowOrientation = 'horizontal' | 'vertical' | 'both';

export function nextIndex(
	key: string,
	current: number,
	count: number,
	orientation: ArrowOrientation = 'both'
): number | null {
	if (count <= 0) return null;
	const forward =
		(orientation !== 'vertical' && key === 'ArrowRight') ||
		(orientation !== 'horizontal' && key === 'ArrowDown');
	const backward =
		(orientation !== 'vertical' && key === 'ArrowLeft') ||
		(orientation !== 'horizontal' && key === 'ArrowUp');
	if (forward) return (current + 1) % count;
	if (backward) return (current - 1 + count) % count;
	if (key === 'Home') return 0;
	if (key === 'End') return count - 1;
	return null;
}
