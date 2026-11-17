import { CheckIcon as Check } from '@kition/tessera-icons/react';
import { useTranslation } from 'react-i18next';
import { noop } from 'lodash-es';
import { forwardRef, useImperativeHandle, useRef, useState } from 'react';
import type { ForwardRefRenderFunction } from 'react';
import type { ISelectCell } from '../../renderers';
import type { IEditorProps, IEditorRef } from './EditorContainer';
import { resolveSelectChoiceColors } from '../../utils/selectChoiceColors';
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandList, CommandListItem } from '@kition/kit-editor';

const SelectEditorBase: ForwardRefRenderFunction<
  IEditorRef<ISelectCell>,
  IEditorProps<ISelectCell>
> = (props, ref) => {
  const { cell, isEditing, style, onChange, setEditing, theme } = props;
  const { t } = useTranslation('table');
  const { data, isMultiple, choiceSorted = [], choiceMap = {} } = cell;
  const [values, setValues] = useState(data);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useImperativeHandle(ref, () => ({
    focus: () => inputRef.current?.focus(),
    setValue: (data: ISelectCell['data']) => setValues(data),
    saveValue: noop,
  }));

  const onSelect = (v: string, id?: string) => {
    const existIndex = values.findIndex((item) => {
      if (typeof item === 'string') return item === v;
      return item.title === v;
    });
    const newCellValue =
      existIndex > -1
        ? values.filter((_, index) => index !== existIndex)
        : [...values, id ? { id, title: v } : v];
    if (!isMultiple) {
      const value = newCellValue.length ? newCellValue[newCellValue.length - 1] : null;
      setTimeout(() => setEditing?.(false));
      setValues(value ? [value] : []);
      return onChange?.(value);
    }
    const value = newCellValue.length ? newCellValue : null;
    setValues(value || []);
    return onChange?.(value);
  };

  return (
    <Command className="rounded-sm border p-2 shadow-sm" style={style}>
      <CommandInput
        ref={inputRef}
        placeholder={t('selectEditor.search')}
      />
      <CommandList>
        <CommandEmpty>{t('selectEditor.notFound')}</CommandEmpty>
        <CommandGroup aria-valuetext="name">
          {isEditing &&
            choiceSorted.map(({ name, id }) => {
              const choiceColors = resolveSelectChoiceColors(
                choiceMap?.[id] ?? choiceMap?.[name],
                theme,
              );
              return (
                <CommandListItem
                  className="justify-between"
                  key={name}
                  value={name}
                  onSelect={() => onSelect(name, id)}
                >
                  <div
                    className="text-ellipsis whitespace-nowrap rounded-[6px] px-2 text-[12px]"
                    style={choiceColors}
                  >
                    {name}
                  </div>
                  {values?.includes(name) && <Check className={'ml-2 size-4'} />}
                </CommandListItem>
              );
            })}
        </CommandGroup>
      </CommandList>
    </Command>
  );
};

export const SelectEditor = forwardRef(SelectEditorBase);
