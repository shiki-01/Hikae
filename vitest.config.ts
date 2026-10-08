import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

const testDir = (file: string) => fileURLToPath(new URL(`./src/test/${file}`, import.meta.url));

// 純関数のテストは node 環境のまま。コンポーネントのスモークテストは、ファイル先頭の
// `// @vitest-environment jsdom` で jsdom に切り替える
export default defineConfig({
	plugins: [svelte(), svelteTesting()],
	resolve: {
		alias: {
			// SvelteKit の仮想モジュールは、テスト用の代替に差し替える
			'$app/navigation': testDir('app-navigation.ts'),
			'$app/paths': testDir('app-paths.ts'),
			'$app/state': testDir('app-state.ts')
		}
	},
	test: {
		environment: 'node',
		include: ['src/**/*.test.ts'],
		setupFiles: [testDir('setup.ts')]
	}
});
