<script lang="ts">
	interface Props {
		value?: number;
		indeterminate?: boolean;
		state?: 'running' | 'done' | 'failed';
		label: string;
		class?: string;
	}

	let {
		value = 0,
		indeterminate = false,
		state = 'running',
		label,
		class: className = ''
	}: Props = $props();

	const percent = $derived(Math.max(0, Math.min(100, value)));
	const fill = $derived(
		state === 'failed' ? 'bg:state-danger' : state === 'done' ? 'bg:state-saved' : 'bg:accent'
	);
</script>

<div
	role="progressbar"
	aria-label={label}
	aria-valuemin={0}
	aria-valuemax={100}
	aria-valuenow={indeterminate ? undefined : percent}
	class={`w:100% h:4px bg:border r:full overflow:hidden position:relative ${className}`}
>
	{#if indeterminate}
		<div class={`h:100% w:40% r:full motion-slide ${fill}`}></div>
	{:else}
		<div
			class={`h:100% r:full transition:width|var(--duration-base) ${fill}`}
			style:width={`${percent}%`}
		></div>
	{/if}
</div>
