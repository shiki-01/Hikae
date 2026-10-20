<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import { useCloudBackupStatus } from './queries';
	import { cloudStatusMessage } from './cloud-backup-model';

	interface Props {
		projectId: string;
	}

	let { projectId }: Props = $props();

	// 自動保存のクラウド控えの件数（設計書 6.4）。読み取りのみ
	const status = useCloudBackupStatus(() => projectId);
</script>

{#if status.isPending}
	<Skeleton class="h:48px mt:3" />
{:else if status.data}
	{@const message = cloudStatusMessage(status.data)}
	<div class="flex flex-direction:column gap:1 p:3 r:md bg:bg-subtle" role="status">
		<span class="type-body">{t(message.key, message.params)}</span>
		{#if status.data.enabled}
			<span class="type-small fg:fg-muted">{t('settings.cloud_status_note')}</span>
		{/if}
	</div>
{/if}
