<script lang="ts">
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Button from '#lib/components/Button.svelte';
	import { describeError } from '#lib/features/notifications/error-view.js';

	interface Props {
		error: unknown;
		onretry?: () => void;
		onlogin?: () => void;
	}

	let { error, onretry, onlogin }: Props = $props();

	// 3 要素（何が起きたか・データは無事か・次の行動）に分けて出す（docs/design.md 5章）
	const view = $derived(describeError(error, { retry: onretry !== undefined, guide: false }));
</script>

<div role="alert" class="flex flex-direction:column gap:3 p:4">
	<p class="m:0 type-heading fg:state-danger flex align-items:center gap:2">
		<AlertTriangle size={18} aria-hidden="true" />
		<span>{view.title}</span>
	</p>
	{#if view.message}
		<p class="m:0 type-body fg:fg-muted white-space:pre-line">{view.message}</p>
	{/if}
	{#if view.primaryKind === 'retry' && onretry}
		<div>
			<Button variant="secondary" class="min-h:44px" onclick={onretry}>{view.primaryLabel}</Button>
		</div>
	{:else if view.primaryKind === 'login' && onlogin}
		<div>
			<Button class="min-h:44px" onclick={onlogin}>{view.primaryLabel}</Button>
		</div>
	{/if}
</div>
