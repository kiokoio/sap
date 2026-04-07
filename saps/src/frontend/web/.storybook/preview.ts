import type { Preview } from '@storybook/svelte';
import '../src/index.css';

const customViewports = {
  mobileSmall: {
    name: 'Mobile S (320x568)',
    styles: { width: '320px', height: '568px' },
    type: 'mobile',
  },
  mobile: {
    name: 'Mobile M (375x667)',
    styles: { width: '375px', height: '667px' },
    type: 'mobile',
  },
  tablet: {
    name: 'Tablet (768x1024)',
    styles: { width: '768px', height: '1024px' },
    type: 'tablet',
  },
  laptop: {
    name: 'Laptop (1024x768)',
    styles: { width: '1024px', height: '768px' },
    type: 'desktop',
  },
  desktop: {
    name: 'Desktop (1440x900)',
    styles: { width: '1440px', height: '900px' },
    type: 'desktop',
  },
  iphoneSE: {
    name: 'iPhone SE (320x449)',
    styles: { width: '320px', height: '449px' },
    type: 'mobile',
  },
  commonAndroid: {
    name: 'Common Android (360x649)',
    styles: { width: '360px', height: '649px' },
    type: 'mobile',
  },
  iphoneSE3: {
    name: 'iPhone SE (3rd) (375x547)',
    styles: { width: '375px', height: '547px' },
    type: 'mobile',
  },
  iphone15: {
    name: 'iPhone 15 (393x659)',
    styles: { width: '393px', height: '659px' },
    type: 'mobile',
  },
  iphone15Plus: {
    name: 'iPhone 15 Plus (430x739)',
    styles: { width: '430px', height: '739px' },
    type: 'mobile',
  },
  ipadMini6: {
    name: 'iPad Mini (6th) (744x1026)',
    styles: { width: '744px', height: '1026px' },
    type: 'tablet',
  },
  ipad10: {
    name: 'iPad (10th) (820x1073)',
    styles: { width: '820px', height: '1073px' },
    type: 'tablet',
  },
  ipadPro129: {
    name: 'iPad Pro (12.9") (1024x1259)',
    styles: { width: '1024px', height: '1259px' },
    type: 'tablet',
  },
  macBookAir13: {
    name: 'MacBook Air (13") (1280x715)',
    styles: { width: '1280px', height: '715px' },
    type: 'desktop',
  },
  macBookAir15: {
    name: 'MacBook Air (15") (1440x815)',
    styles: { width: '1440px', height: '815px' },
    type: 'desktop',
  },
  macBookPro14: {
    name: 'MacBook Pro (14") (1512x865)',
    styles: { width: '1512px', height: '865px' },
    type: 'desktop',
  },
  macBookPro16: {
    name: 'MacBook Pro (16") (1728x1000)',
    styles: { width: '1728px', height: '1000px' },
    type: 'desktop',
  },
  iMac24: {
    name: 'iMac (24") (2240x1156)',
    styles: { width: '2240px', height: '1156px' },
    type: 'desktop',
  },
  studioDisplay: {
    name: 'Studio Display (2560x1336)',
    styles: { width: '2560px', height: '1336px' },
    type: 'desktop',
  },
  studioDisplayHalf: {
    name: 'Studio Display, half (1278x1336)',
    styles: { width: '1278px', height: '1336px' },
    type: 'desktop',
  },
  proDisplayXDR: {
    name: 'Pro Display XDR (3008x1588)',
    styles: { width: '3008px', height: '1588px' },
    type: 'desktop',
  },
};

const preview: Preview = {
  parameters: {
    actions: { argTypesRegex: '^on[A-Z].*' },
    controls: {
      matchers: { color: /(background|color)$/i, date: /Date$/ },
    },
    viewport: {
      viewports: customViewports,
    },
  },
};

export default preview;
