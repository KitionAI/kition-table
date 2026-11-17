import type { ChangeEvent, ForwardRefRenderFunction, KeyboardEvent, RefObject } from 'react';
import { useState, useRef, useImperativeHandle, forwardRef, useMemo } from 'react';
import { Key } from 'ts-keycode-enum';
import { fromDateInputValue, toDateInputValue } from '../../../lib/dateFormatting';
import { GRID_DEFAULT } from '../../configs';
import type { ILinkCell, INumberCell, ITextCell } from '../../renderers';
import { CellType } from '../../renderers';
import type { IEditorRef, IEditorProps } from './EditorContainer';
import { TableDateCellEditor } from './TableDateCellEditor';
import { TextInput, useAutoSize } from '@kition/kit-editor';

const { rowHeight: defaultRowHeight } = GRID_DEFAULT;

const TextEditorBase: ForwardRefRenderFunction<
  IEditorRef<ITextCell | INumberCell>,
  IEditorProps<ITextCell | INumberCell | ILinkCell>
> = (props, ref) => {
  const { cell, rect, style, theme, isEditing, onChange, setEditing } = props;
  const { cellLineColorActived } = theme;
  const { width, height } = rect;
  const { displayData, type } = cell;
  const inputType = type === CellType.Text ? (cell.inputType ?? 'text') : 'text';
  const usesDateInput = inputType === 'date' || inputType === 'datetime-local';
  const editingData = type === CellType.Text
    ? usesDateInput
      ? toDateInputValue(inputType, cell.data)
      : cell.data
    : displayData;
  const needWrap = (cell as ITextCell)?.isWrap;
  const inputRef = useRef<HTMLInputElement | HTMLTextAreaElement | HTMLDivElement>(null);
  const [value, setValueInner] = useState(editingData);

  useImperativeHandle(ref, () => ({
    focus: () => inputRef.current?.focus(),
    setValue: (value: string | number | null | undefined) => setValueInner(
      usesDateInput ? toDateInputValue(inputType, value) : String(value ?? ''),
    ),
    saveValue,
  }));

  const saveValue = () => {
    if (value === editingData || !isEditing) return;
    if (type === CellType.Number) {
      onChange?.(Number(value));
    } else if (usesDateInput) {
      onChange?.(fromDateInputValue(inputType, String(value ?? '')));
    } else {
      onChange?.(typeof value === 'string' ? value.trim() : value);
    }
  };

  const onChangeInner = (e: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
    const value = e.target.value;
    setValueInner(value);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    const { keyCode, shiftKey } = event;
    if (keyCode === Key.Enter && !shiftKey) {
      event.preventDefault();
    }
    if (keyCode === Key.Enter && shiftKey) {
      event.stopPropagation();
    }
  };

  const attachStyle = useMemo(() => {
    const style: React.CSSProperties = {
      width: width + 4,
      minHeight: height + 4,
      height: needWrap ? 'auto' : height + 4,
      marginLeft: -2,
      marginTop: -2,
      textAlign: cell.contentAlign ?? (type === CellType.Number ? 'right' : 'left'),
    };
    return style;
  }, [cell.contentAlign, type, height, width, needWrap]);
  // A row taller than two lines keeps the text being typed (and its caret) centred, as the grid
  // draws it; a short row opens a two-line box from the top.
  const roomyRow = height > defaultRowHeight * 2;
  // A long text grows between two (one in a tall row) and five lines as it is edited.
  useAutoSize(
    inputRef as RefObject<HTMLTextAreaElement>,
    value,
    needWrap ? { minRows: roomyRow ? 1 : 2, maxRows: 5 } : null,
  );

  if (usesDateInput) {
    return (
      <TableDateCellEditor
        ref={inputRef as RefObject<HTMLDivElement>}
        active={Boolean(isEditing)}
        inputType={inputType}
        value={String(value ?? '')}
        style={{
          border: `2px solid ${cellLineColorActived}`,
          ...style,
          ...attachStyle,
        }}
        onValueChange={setValueInner}
        onCommit={(next) => {
          onChange?.(fromDateInputValue(inputType, next));
          setTimeout(() => setEditing?.(false));
        }}
      />
    );
  }

  return (
    <>
      {needWrap ? (
        <div
          style={{
            ...style,
            ...attachStyle,
            paddingBottom: 16,
            ...(roomyRow
              ? { display: 'flex', flexDirection: 'column', justifyContent: 'center', paddingTop: 16 }
              : null),
            border: `2px solid ${cellLineColorActived}`,
          }}
          className="relative rounded-md bg-background"
        >
          <textarea
            ref={inputRef as RefObject<HTMLTextAreaElement>}
            style={{ textAlign: attachStyle.textAlign }}
            className={`w-full cursor-text resize-none rounded border-none bg-background px-2 ${roomyRow ? 'py-1' : 'pt-1'} text-[13px] leading-[1.4rem] focus-visible:outline-none`}
            value={value}
            onBlur={saveValue}
            onKeyDown={onKeyDown}
            onChange={onChangeInner}
          />
          <div className="absolute bottom-[2px] left-0 w-full rounded-b-md bg-background pb-[2px] pr-1 text-right text-xs text-slate-400 dark:text-slate-600">
            Shift + Enter
          </div>
        </div>
      ) : (
        <TextInput
          ref={inputRef as RefObject<HTMLInputElement>}
          style={{
            border: `2px solid ${cellLineColorActived}`,
            paddingInline: 6,
            boxShadow: 'none',
            ...style,
            ...attachStyle,
          }}
          value={value}
          className="cursor-text border-2 text-[13px]"
          onChange={onChangeInner}
          onBlur={saveValue}
          onMouseDown={(e) => e.stopPropagation()}
        />
      )}
    </>
  );
};

export const TextEditor = forwardRef(TextEditorBase);
