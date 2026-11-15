# ADR 0003: BIBFRAME completo da Library of Congress, sem perfil nem tipo de item

- **Status:** aceito (diretriz obrigatória de Paulo em 2026-11-14). Substitui a decisão 3 do [ADR 0002](0002-hub-relacoes-e-categoria.md) (`material_type`) e o item 12 do [modelo bibliográfico](../specs/catalogo/modelo-bibliografico.md) (`extra`).
- **Spec:** [bibframe-completo.md](../specs/catalogo/bibframe-completo.md).

## Contexto

O modelo era "inspirado em BIBFRAME 2.0": as entidades certas, um subconjunto das propriedades, uma categoria de material própria (9 valores), tipo de item herdado do Koha e formulários que mostravam um perfil por categoria com o resto atrás de "expandir". Paulo determinou que isso não basta.

## A diretriz (resumo fiel do texto de Paulo)

1. Implementar a ontologia BIBFRAME da LC (`http://id.loc.gov/ontologies/bibframe/`) pura e completa: sem modificação, sem perder propriedade padrão, sem esconder campo oficial só na cópia do MARC.
2. Tipo de material vem das classes e listas oficiais (`bf:content`, `bf:media`, `bf:carrier`, subclasses de `bf:Work` e `bf:Instance`), numa caixa `<select>`, inclusive na grade. Nada de sigla inventada (BK, RO, GB) nem de perfil customizado.
3. Ícones SVG leves, inline, por categoria semântica, em todo lugar onde um atributo categórico é escolhido ou mostrado: conteúdo, suporte e mídia, agentes, público, gênero, situação e política de uso do exemplar.
4. Todas as propriedades de `bf:Work`, `bf:Instance` e `bf:Item` no modelo, no formulário (sem "expandir") e na exportação RDF/XML e Turtle.
5. Unidades com cadastro próprio (`bf:Organization`, `bf:heldBy`); localizações (`bf:sublocation`) e coleções como listas simples por unidade, com nome legível e identificador gerado pelo sistema.
6. Política de empréstimo ligada ao exemplar por `bf:usageAndAccessPolicy`, separada da descrição.

## Decisão

1. A ontologia entra no repositório como arquivo fixado e é a fonte gerada da lista de classes e propriedades; a completude é conferida por teste em três camadas (modelo, formulário, exportação).
2. Versão fixada: 3.0.1, a publicada no endereço da diretriz.
3. Armazenamento relacional: tabelas tipadas para o que é consultado, tabela de declarações para o resto. `extra` e `material_type` saem.
4. Tipo de item sai do modelo. Os códigos do Koha vivem só numa tabela de correspondência da borda MARC, porque a exportação MARC é área de paridade.
5. Os formulários deixam de ter perfil por categoria e seção recolhida.

## Consequências

- As telas e o comando de tabelas de códigos (PRs #50 a #52) são substituídos na etapa 4 da spec; até lá continuam funcionando.
- A spec de interação perde o foco 80/20 dentro do bloco de edição, os perfis (§4.2) e "Todos os campos" (§20 item 10). O resto dela continua.
- O formulário fica longo; a ordem fixa das seções e o teclado são o que o mantém usável.
- A tarefa 2.11 cresce (Turtle além de RDF/XML) e passa a ter teste de cobertura.

## Alternativas descartadas

- **Manter o subconjunto e ampliar aos poucos, sem teste de cobertura:** não garante "nenhuma propriedade perdida".
- **Uma coluna por propriedade:** 231 propriedades, a maioria rara; a tabela de declarações guarda o mesmo com leitura única.
- **Triple store:** contraria a decisão 5 do doc 04 e não é exigido pela diretriz.
