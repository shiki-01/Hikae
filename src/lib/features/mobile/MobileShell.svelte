<script lang="ts">
	import { useQueryClient } from '@tanstack/svelte-query';
	import { t } from '#lib/i18n/index.js';
	import Spinner from '#lib/components/Spinner.svelte';
	import ErrorNotice from './ErrorNotice.svelte';
	import FilesScreen from './FilesScreen.svelte';
	import LoginScreen from './LoginScreen.svelte';
	import MobileTabBar from './MobileTabBar.svelte';
	import RepoPicker from './RepoPicker.svelte';
	import SettingsScreen from './SettingsScreen.svelte';
	import UploadScreen from './UploadScreen.svelte';
	import { nav } from './nav.svelte';
	import { mobileKeys, useMobileAccount } from './queries';

	// モバイル版の画面の入口（docs/design.md 14.7）。ログイン → 保存先の選択 → タブ（ファイル・アップロード・設定）の順
	const client = useQueryClient();
	const account = useMobileAccount();

	async function onLogged(): Promise<void> {
		nav.needsLogin = false;
		await client.invalidateQueries({ queryKey: mobileKeys.account });
		await client.invalidateQueries({ queryKey: ['mobile'] });
	}
</script>

<div class="flex flex-direction:column h:100% min-h:0 bg:bg fg:fg">
	{#if account.isPending}
		<div class="flex flex:1 align-items:center justify-content:center gap:2 fg:fg-muted">
			<Spinner />
			<span class="type-small">{t('a11y.loading')}</span>
		</div>
	{:else if account.isError}
		<ErrorNotice
			error={account.error}
			onretry={() => void account.refetch()}
			onlogin={() => (nav.needsLogin = true)}
		/>
	{:else if account.data === null || account.data === undefined || nav.needsLogin}
		<LoginScreen onlogged={() => void onLogged()} />
	{:else if nav.repo === null || nav.choosingRepo}
		<RepoPicker />
	{:else}
		<div class="flex flex-direction:column flex:1 min-h:0">
			{#if nav.tab === 'files'}
				<FilesScreen />
			{:else if nav.tab === 'upload'}
				<UploadScreen />
			{:else}
				<SettingsScreen />
			{/if}
		</div>
		<MobileTabBar />
	{/if}
</div>
