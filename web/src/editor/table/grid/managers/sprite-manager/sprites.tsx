import { GripVerticalIcon as DraggableHandle, Maximize2Icon as Maximize2, PlusIcon as Plus, XIcon as X, ChevronDownIcon as ChevronDown, ChevronRightIcon as ChevronRight, LockIcon as Lock, EyeOffIcon as EyeOff, SparklesIcon as Sparkles, StarIcon as Star, HeartIcon as Heart, ThumbsUpIcon as ThumbsUp } from '@kition/tessera-icons/react';
import { renderToString } from 'react-dom/server';

export interface ISpriteProps {
  fgColor: string;
  bgColor: string;
}

const drag = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<DraggableHandle style={{ color: fgColor }} />);
};

const detail = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<Maximize2 style={{ color: fgColor }} />);
};

const add = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<Plus style={{ color: fgColor }} />);
};

const close = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<X style={{ color: fgColor }} />);
};

const expand = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<ChevronDown style={{ color: fgColor }} />);
};

const collapse = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<ChevronRight style={{ color: fgColor }} />);
};

const lock = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<Lock style={{ color: fgColor }} />);
};

const ai = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<Sparkles style={{ color: fgColor }} />);
};

export const eyeOff = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<EyeOff style={{ color: fgColor }} />);
};

// Rating steps: filled in the step's color (the renderer passes the highlight, hover or line color).
const star = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<Star style={{ color: fgColor }} fill="currentColor" />);
};

const heart = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<Heart style={{ color: fgColor }} fill="currentColor" />);
};

const thumbs = (props: ISpriteProps) => {
  const { fgColor } = props;
  return renderToString(<ThumbsUp style={{ color: fgColor }} fill="currentColor" />);
};

export const sprites = {
  add,
  drag,
  detail,
  close,
  expand,
  collapse,
  lock,
  ai,
  eyeOff,
  star,
  heart,
  thumbs,
};

export enum GridInnerIcon {
  Add = 'add',
  Drag = 'drag',
  Detail = 'detail',
  Close = 'close',
  Expand = 'expand',
  Collapse = 'collapse',
  Lock = 'lock',
  AI = 'ai',
  EyeOff = 'eyeOff',
  Star = 'star',
  Heart = 'heart',
  Thumbs = 'thumbs',
}
