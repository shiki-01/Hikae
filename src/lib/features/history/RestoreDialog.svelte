<script lang="ts">
	import { ShieldCheck } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatDateTime } from '#lib/i18n/format.js';
	import type { RestoreScope, SavePoint } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import ImpactList from '#lib/components/ImpactList.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import { useImpact } from './queries';

	interface Props {
		open: boolean;
		projectId: string;
		point: SavePoint | null;
		scope: RestoreScope;
		pending: boolean;
		onconfirm: () => void;
		oncancel: () => void;
	}

	let { open, projectId, point, scope, pending, onconfirm, oncancel }: Props = $props();

	const impact = useImpact(
		() => projectId,
		() => (open && point ? point.id : null),
		() => scope
	);

	const timeLabel = $derived(point ? formatDateTime(point.createdAt) : '');
	const title = $derived(
		scope.kind === 'file'
			? t('restore.title_file', { name: scope.path, time: timeLabel })
			: t('restore.title', { time: timeLabel })
	);
	const items = $derived(impact.data ?? []);
</script>

<Dialog {open} {title} variant="confirm" busy={pending} onclose={oncancel}>
	<div class="flex flex-direction:column gap:3">
		{#if impact.isPending}
			<div class="flex flex-direction:column gap:2" aria-busy="true">
				<Skeleton class="h:40px" />
				<Skeleton class="h:40px" />
			</div>
		{:else if items.length === 0}
			<p class="m:0 type-body fg:fg-muted">{t('restore.no_impact')}</p>
		{:else}
			<p class="m:0 type-body">{t('restore.impact_heading', { count: items.length })}</p>
			<ImpactList {items} {timeLabel} />
		{/if}
	</div>

	{#snippet footerNote()}
		<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle">
			<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
			<p class="m:0 type-body">
				{t('restore.auto_save_notice')}
				<span class="fg:fg-muted">{t('restore.auto_save_detail')}</span>
			</p>
		</div>
	{/snippet}

	{#snippet actions()}
		<Button variant="secondary" disabled={pending} onclick={oncancel}>{t('restore.cancel')}</Button>
		<Button
			variant="primary"
			loading={pending}
			disabled={impact.isPending || items.length === 0}
			onclick={onconfirm}
		>
			{t('restore.button')}
		</Button>
	{/snippet}
</Dialog>
