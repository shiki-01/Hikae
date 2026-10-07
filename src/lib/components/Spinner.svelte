<script lang="ts">
	import { LoaderCircle } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';

	interface Props {
		size?: 'sm' | 'md';
		label?: string;
		class?: string;
	}

	let { size = 'md', label, class: className = '' }: Props = $props();

	const pixels = $derived(size === 'sm' ? 16 : 24);
</script>

<span role="status" class={`inline-flex align-items:center fg:fg-muted ${className}`}>
	<LoaderCircle size={pixels} class="motion-spin" aria-hidden="true" />
	<span class="sr-only">{label ?? t('a11y.loading')}</span>
</span>
