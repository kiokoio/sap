import type { Meta, StoryObj } from '@storybook/svelte';
import App from '../App.svelte';

const meta = {
  title: 'App/Placeholder',
  component: App,
} satisfies Meta<App>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
