<script lang="ts">
	import { ChevronDown, ChevronRight, Copy } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import { closeError, notifications } from './store.svelte';

	let showDetails = $state(false);
	let copied = $state(false);

	const current = $derived(notifications.error);

	$effect(() => {
		if (!current) {
			showDetails = false;
			copied = false;
		}
	});

	async function copy(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			copied = false;
		}
	}

	function primary() {
		const state = notifications.error;
		if (!state) return;
		const { view, onretry, onprimary } = state;
		closeError();
		if (view.primaryKind === 'retry') onretry?.();
		else if (view.primaryKind === 'copy' && view.copyText) void copy(view.copyText);
		onprimary?.();
	}

	function secondary() {
		const state = notifications.error;
		if (!state) return;
		closeError();
		state.onsecondary?.();
	}
</script>

<Dialog
	open={current !== null}
	variant="error"
	title={current?.view.title ?? ''}
	onclose={closeError}
>
	{#if current}
		{@const view = current.view}
		<div class="flex flex-direction:column gap:4">
			<p class="m:0 type-body white-space:pre-line">{view.message}</p>

			<div class="flex flex-direction:column gap:2">
				<button
					type="button"
					aria-expanded={showDetails}
					onclick={() => (showDetails = !showDetails)}
					class="inline-flex align-items:center gap:1 align-self:flex-start p:0 b:0 bg:transparent fg:fg-muted type-small cursor:pointer fg:fg:hover"
				>
					{#if showDetails}
						<ChevronDown size={14} aria-hidden="true" />
						{t('error.hide_details')}
					{:else}
						<ChevronRight size={14} aria-hidden="true" />
						{t('error.show_details')}
					{/if}
				</button>
				{#if showDetails}
					<div class="flex flex-direction:column gap:2 p:3 r:md bg:bg-subtle b:1px|solid|border">
						<pre class="m:0 type-mono white-space:pre-wrap overflow-wrap:anywhere">{view.code ===
								'generic' || view.code === 'backend'
								? ''
								: `${view.code}\n`}{view.technical}</pre>
						<div class="flex align-items:center gap:2 justify-content:end">
							{#if copied}
								<span class="type-small fg:fg-muted" role="status">{t('error.copied')}</span>
							{/if}
							<Button
								size="sm"
								variant="secondary"
								onclick={() =>
									copy(
										view.code === 'generic' || view.code === 'backend'
											? view.technical
											: `${view.code}\n${view.technical}`
									)}
							>
								<Copy size={14} aria-hidden="true" />
								{t('error.copy_details')}
							</Button>
						</div>
					</div>
				{/if}
			</div>
		</div>
	{/if}
	{#snippet actions()}
		{#if current}
			{@const view = current.view}
			{@const closeOnly = view.primaryKind === 'close' && !current.onprimary}
			{#if view.secondaryLabel}
				<Button variant="secondary" onclick={secondary}>{view.secondaryLabel}</Button>
			{/if}
			{#if !closeOnly}
				<Button variant="secondary" onclick={closeError}>{t('error.close')}</Button>
			{/if}
			<Button onclick={primary}>
				{closeOnly ? t('error.close') : view.primaryLabel}
			</Button>
		{/if}
	{/snippet}
</Dialog>
