# BIBFRAME completo da Library of Congress: modelo, tipos de material, formulários e exportação (v0)

> Status: v0, plano. Diretriz obrigatória de Paulo em 2026-11-14 (texto integral em [ADR 0003](../../adr/0003-bibframe-completo.md)). Substitui, onde diverge: [modelo bibliográfico](modelo-bibliografico.md) itens 8, 12 e 16, [tabelas de códigos](tabelas-de-codigos.md), a [spec de interação](../../UI_BIBFRAME_INTERACTION_SPEC.md) §3 (foco 80/20), §4.2 (perfis por categoria) e §20 item 10 ("Todos os campos"), e a decisão 5 do [doc 04](../../arquitetura/04-decisoes.md) ("inspirado em"). Este documento não traz código: define o alvo, como a cobertura é conferida e a ordem dos PRs.

A diretriz, em quatro frases:

- O Elzevir implementa a ontologia BIBFRAME da Library of Congress **inteira e sem modificação**: toda classe e toda propriedade oficial existe no modelo, no formulário e na exportação RDF.
- **Nada de sigla inventada** para tipo de material (BK, RO, GB) nem de perfil por categoria: o tipo vem das classes e dos vocabulários oficiais, numa caixa de seleção, com ícone.
- **Nada de "expandir"** que esconda propriedade oficial.
- **Descrição e regra de empréstimo não se misturam**: a política de circulação se liga ao exemplar por `bf:usageAndAccessPolicy`.

## 1. Comportamento

### A ontologia é a fonte, e a cobertura é conferida por teste

1. A versão é fixada num arquivo do repositório: `reference/bibframe/bibframe.rdf`, cópia sem alteração do arquivo oficial (licença CC0), com a versão e o hash em `reference/bibframe/README.md`. A versão publicada hoje em `http://id.loc.gov/ontologies/bibframe/` é a **3.0.1** (3 de dezembro de 2025): 224 classes e 231 propriedades. (A v0 desta spec dizia 226 e 235: eram os números do arquivo em desenvolvimento no GitHub da LC, que já tem mudanças ainda não publicadas. Vale o arquivo publicado.) Ela contém tudo o que a diretriz cita (`bf:Hub`, `bf:Serial` como subclasse de `bf:Work`, `bf:hasExpression`, `bf:ShelfMarkUdc`).
2. `perl tools/bibframe-extractor/extract.pl` lê esse arquivo e escreve `data/loc/bibframe.json`; `cargo xtask codegen` gera daí, em `elzevir-data` (`elzevir_data::bibframe`), a lista tipada de classes e propriedades: nome, rótulo, definição, domínio, contradomínio, superclasse, propriedade inversa e o uso sugerido. Ninguém escreve essa lista à mão; o CI roda `extract.pl --check` e `cargo xtask codegen --check`, que falham se o gerado não bate com o arquivo.
3. **Teste de cobertura**, um por camada. Cada um percorre a lista gerada e falha citando a propriedade que faltou:
   - **modelo:** toda propriedade aplicável a Hub, Obra, Instância ou Exemplar tem onde ser gravada e lida;
   - **formulário:** toda propriedade aplicável à entidade tem campo no formulário dela;
   - **exportação:** toda propriedade gravada sai em RDF/XML e em Turtle com o IRI oficial, e volta igual na leitura do que foi exportado.
4. Uma propriedade é **aplicável** a uma entidade quando o domínio dela é a classe da entidade ou uma superclasse, ou quando a ontologia não declara domínio (118 das 231; por exemplo `bf:title`, `bf:subject`, `bf:note`, `bf:genreForm`) e o comentário oficial "Suggested use - With Work or Instance" cita a classe. Só 24 propriedades não têm nem domínio nem esse comentário (`bf:identifiedBy`, `bf:note`, `bf:status`, `bf:date`...): para elas, uma tabela escrita à mão (`GENERIC_USE` em `crates/elzevir-data/src/bibframe.rs`) diz a quais entidades se aplicam, seguindo a conversão MARC → BIBFRAME da LC. O Hub, que os comentários não citam, recebe também `bf:title` e `bf:contribution`, como nos Hubs que a LC publica. Pela regra, são 12 propriedades no Hub, 119 na Obra, 132 na Instância e 33 no Exemplar; as demais descrevem recursos aninhados (título, nota, contribuição, atividade de provisão).
   Enquanto as etapas não terminam, cada teste tem a lista do que falta num arquivo (para o modelo, `crates/elzevir-store/tests/data/bibframe-model-missing.txt`: 263 de 296 na etapa 1). O teste falha se a lista não bate com o que o modelo guarda, e um limite numérico no teste só pode baixar.
5. Atualizar a versão da ontologia é um PR que troca o arquivo: os testes de cobertura dizem o que faltou.

### Modelo

6. Continua relacional (doc 04, decisão 5: sem triple store), com **nenhuma etiqueta MARC no núcleo**. Duas formas de guardar, a mesma leitura:
   - **tabelas tipadas** para o que a circulação, a busca e a listagem consultam (títulos, contribuições, assuntos, classificação, identificadores, atividades de provisão, exemplares);
   - **tabela de declarações** (`statements`: entidade, propriedade pelo número gerado, ordem, e o valor: literal, termo de vocabulário, outra entidade ou um recurso aninhado) para todas as outras propriedades.
7. A coluna `extra` (JSON) deixa de existir: o que estava nela vira declaração.
8. A classe de cada entidade é guardada: a Obra tem uma ou mais subclasses de `bf:Work` (21 ao todo: `bf:Text`, `bf:Audio`, `bf:NotatedMusic`, `bf:MovingImage`, `bf:StillImage`, `bf:Cartography`, `bf:Object`, `bf:Multimedia`, `bf:Dataset`, `bf:MixedMaterial`, `bf:NotatedMovement`, `bf:Manuscript`, `bf:Arrangement`, `bf:Monograph`, `bf:Serial`, `bf:Series`, `bf:Collection`, `bf:Integrating`, e ainda `bf:MusicAudio` e `bf:NonMusicAudio` sob `bf:Audio` e `bf:Kit` sob `bf:Multimedia`); a Instância, subclasses de `bf:Instance` (`bf:Print`, `bf:Electronic`, `bf:Tactile`, `bf:Microform`, `bf:Archival`). O `enum` `material_type` (ADR 0002) é substituído por isso.
9. Agentes têm as cinco subclasses oficiais (`bf:Person`, `bf:Organization`, `bf:Meeting`, `bf:Family`, `bf:Jurisdiction`); identificadores, as 48 de `bf:Identifier`; classificação, `bf:ClassificationDdc`, `Udc`, `Lcc`, `Nlm`, `Nal`; número de chamada, `bf:ShelfMarkDdc`, `Udc`, `Lcc`, `Nlm` e `bf:ShelfMark`; atividade de provisão, `bf:Publication`, `Production`, `Distribution`, `Manufacture`, `Modification`.
10. O Cutter PHA não é classe da ontologia: é a parte do item de `bf:ClassificationDdc` (`bf:itemPortion`) e do `bf:ShelfMarkDdc` do exemplar, com a tabela PHA indicada em `bf:source`.

### Declarações (etapa 3a)

10a. Cada propriedade que não tem lugar próprio numa tabela tipada é gravada como **declaração** (`statements`): o sujeito (Hub, Obra, Instância ou Exemplar), a propriedade, a ordem e o valor. Que propriedades têm lugar próprio é uma lista só, no domínio (`TYPED`); o comando de declarações recusa essas, para que nada fique gravado em dois lugares.
10b. O valor é um de cinco tipos, conforme a ontologia:
    - **literal**, com idioma opcional, só para propriedade de dado (`owl:DatatypeProperty`);
    - **conceito** de vocabulário da LC (vocabulário e código), só para propriedade que tem lista (item 11) e só de uma das listas dela;
    - **entidade** do catálogo (outra Obra, Instância, Exemplar ou Hub), para as relações;
    - **IRI** externo, como uma autoridade de `id.loc.gov`;
    - **recurso aninhado**: uma classe da ontologia, um rótulo opcional e as suas próprias declarações (uma `bf:Publication` com lugar, agente e data; um `bf:Title` com `bf:mainTitle`).
    Os três últimos só para propriedade de objeto.
10c. A regra é uma função pura do domínio, que o comando chama: a propriedade existe na ontologia; no primeiro nível, descreve a entidade (item 4); o tipo de valor combina com o tipo da propriedade; o conceito é de uma das listas da propriedade; a classe do recurso existe.
10d. Limites de entrada: 4 níveis de recurso aninhado, 2.000 declarações por sujeito, 10.000 caracteres por literal e 2.000 por IRI.
10e. `catalog.set_statements` troca todas as declarações de um sujeito de uma vez, se ele ainda está na versão lida (`Stale` como na edição). Sobe o `row_version` do sujeito; numa Instância ou num Exemplar, sobe também a versão do registro e pede a reindexação, como a edição já faz.
10f. Propriedade e classe são guardadas por número (tabela `ontology_names`), e conceito também (`lc_concepts`, vocabulário e código), como o resto do vocabulário (modelo bibliográfico, item 8).
10g. A cobertura do modelo (item 3) passa a exigir que toda propriedade aplicável a cada entidade ou tenha lugar tipado, ou volte igual depois de gravada como declaração. A lista do que faltava fica vazia.

### Vocabulários da LC

11. Os valores de propriedades categóricas vêm das listas oficiais de `id.loc.gov/vocabulary/`, copiadas para `reference/bibframe/vocabulary/` (domínio público) com código, IRI e rótulo em inglês; o rótulo em português vem do Fluent:
    - `contentTypes` (`bf:content`), `mediaTypes` (`bf:media`), `carriers` (`bf:carrier`), `issuance` (`bf:issuance`);
    - `relators` (`bf:role`), `languages`, `countries`;
    - `maudience` (`bf:intendedAudience`), `mstatus` (`bf:status`) e as demais listas que a ontologia aponta como contradomínio.
    Como está feito (etapa 2): os três arquivos em lote de `id.loc.gov/download` (`cataloging`, `languages`, `countries`, SKOS em N-Triples) ficam em `reference/loc-vocabularies/`, sem alteração; `tools/bibframe-extractor/vocabularies.pl` escreve `data/loc/vocabularies.json` e o `cargo xtask codegen` gera `elzevir_data::vocabulary`: 57 vocabulários e 2.205 conceitos, cada um com código, IRI, rótulo em inglês e a marca de descontinuado (lido, nunca oferecido). Que propriedade usa que vocabulário é a única tabela escrita à mão (`PROPERTY_SCHEMES`, 40 propriedades). Nove ligações são as que a conversão `marc2bibframe2` da LC escreve (`content`, `media`, `carrier`, `language`, `place`, `role`, `status`, `descriptionConventions`, `variantType`); as demais, de características físicas, vêm de modelos genéricos da conversão e foram ligadas pelo nome da classe e pela nota de origem MARC de cada conceito. `bf:genreForm` não tem lista daqui: os gêneros vêm da LCGFT (item 12). Os 8 vocabulários que nenhuma propriedade do BIBFRAME usa ficam numa lista com o motivo (`UNUSED_SCHEMES`; por exemplo, os da extensão BFLC), e um teste exige que todo vocabulário baixado esteja numa das duas listas.
12. `bf:genreForm` usa os termos da LCGFT que a biblioteca escolher (a lista inteira tem milhares) e aceita termo local com rótulo. Romance, HQ/Gibi, Poesia, Teatro e Biografia entram aqui, na Obra.
13. Nenhuma tela aceita código digitado para essas propriedades: é caixa de seleção sobre a lista oficial.

### Tipo de material: caixa de seleção e ícones

14. O tipo de material é o conjunto classe da Obra + `bf:content`, classe da Instância + `bf:media` + `bf:carrier`, e `bf:issuance`. Cada um é um `<select>` com as opções oficiais, na catalogação e na grade.
15. Todo valor categórico mostrado (formulário, listagem, pílulas, grade, OPAC) leva o ícone da categoria, em SVG inline, de um arquivo único de símbolos:

    | Categoria | Valor | Ícone |
    |---|---|---|
    | Conteúdo | `bf:Text` | livro |
    | | `bf:NotatedMusic` | partitura |
    | | `bf:Audio` (inclui palavra falada, `spw`) | fone de ouvido |
    | | `bf:MovingImage` | rolo de filme |
    | | `bf:StillImage` | quadro |
    | | `bf:Serial` | revista |
    | | texto tátil (`tct`) e `bf:Tactile` | braille |
    | | `bf:Cartography` | mapa |
    | | `bf:Object` | cubo |
    | | `bf:Multimedia` | camadas |
    | | `bf:Dataset` | banco de dados |
    | Suporte | `bf:Print`, volume (`nc`) | livro impresso |
    | | `bf:Electronic`, recurso online (`cr`) | tela e nuvem |
    | | disco de áudio (`sd`), videodisco (`vd`) | disco |
    | Agente | `bf:Person`, `bf:Organization`, `bf:Meeting` | pessoa, prédio, evento |
    | Público | infantil, juvenil, adulto | selo com ícone |
    | Gênero | HQ/Gibi, Poesia, Teatro, Biografia e os demais em uso | selo com ícone |
    | Exemplar | `bf:status`: disponível, emprestado, danificado, extraviado | indicador |
    | | `bf:usageAndAccessPolicy`: consulta local | cadeado |

16. O ícone nunca é a única informação: vem com o rótulo ou com `aria-label` (spec de interação §12).
17. "Emprestado" é lido da circulação, não é descrição gravada no exemplar.

### Exemplar, unidade, localização e coleção

18. O exemplar tem todas as propriedades de `bf:Item`: `bf:itemOf`, `bf:identifiedBy` (`bf:Barcode`), `bf:shelfMark`, `bf:heldBy`, `bf:sublocation`, `bf:physicalLocation`, `bf:enumerationAndChronology`, `bf:status`, `bf:note`, `bf:immediateAcquisition`, `bf:electronicLocator` e `bf:usageAndAccessPolicy`.
19. **Unidades** têm cadastro próprio: cada uma é um `bf:Organization`, com nome e dados de contato, e é o valor de `bf:heldBy`.
20. **Localizações** (`bf:sublocation`) e **coleções** são listas simples por unidade. A pessoa escreve só o nome; o sistema gera o identificador interno.
21. **Tipo de item deixa de existir.** O que ele misturava vai para o lugar certo:

    | O tipo de item dizia | Onde fica |
    |---|---|
    | o que a coisa é (livro, DVD, audiolivro, braille) | classe e `bf:content` da Obra; classe, `bf:media` e `bf:carrier` da Instância |
    | gênero (romance, gibi) | `bf:genreForm` da Obra |
    | público (infantil, juvenil) | `bf:intendedAudience` da Obra |
    | como empresta | `bf:usageAndAccessPolicy` do exemplar, que aponta uma política de circulação |

22. A política de circulação é cadastro da circulação (Fase 3: prazo, renovações, multa), com nome legível. A descrição bibliográfica não a conhece além da ligação do item 21.
23. A tela `/equipe/admin/codigos` e a tabela de tipos de item ([tabelas de códigos](tabelas-de-codigos.md)) são retiradas na etapa 4. No lugar entram as telas de Unidades, Localizações e Coleções, só com nomes.

### Formulários

24. O formulário de cada entidade mostra **todas** as propriedades aplicáveis a ela, sempre, agrupadas em seções fixas na ordem da ontologia (título, conteúdo, contribuições, assuntos, classificação, relações, notas, metadados administrativos; para a Instância: título, responsabilidade, edição, provisão, descrição física, mídia e suporte, identificadores, série, notas, localizador eletrônico, metadados administrativos).
25. Não há seção recolhida, botão "expandir" nem perfil por categoria: escolher `bf:Text` não esconde as propriedades de música. Campo vazio não impede gravar.
26. Propriedade repetível tem "+" na própria linha; recurso aninhado (uma `bf:Publication` com lugar, agente e data) é um grupo de campos.
27. Os atalhos de teclado e a regra de `Esc` da spec de interação continuam valendo.

### Exportação e borda MARC

28. Exportação por registro e em lote, em **RDF/XML e Turtle**, com os IRIs oficiais de classes, propriedades e vocabulários (tarefa 2.11, ampliada).
29. O MARC continua só na borda. A conversão MARC21 → BIBFRAME segue a especificação de conversão da LC (`marc2bibframe2`), campo a campo, e passa a preencher o que hoje fica só no registro de origem (títulos variantes, série, todas as notas, atividades de provisão, gênero, público).
30. A exportação MARC para o Koha é área de paridade e precisa dos códigos do Koha (`952 $a`, `$b`, `$c`, `$8`, `$y`, `942 $c`). Eles ficam **só na borda**: uma tabela de correspondência da borda liga unidade, localização, coleção e política ao código do Koha. A importação preenche essa tabela; o núcleo e as telas não a veem.

### Migração dos dados que já existem

31. Uma migração do banco converte o que há, sem perda:
    - `material_type` → classe da Obra e da Instância, `bf:content`, `bf:media`, `bf:carrier`;
    - tipo de item → política de circulação de mesmo nome, mais gênero, público e classe quando o nome do tipo diz (tabela de conversão revisada por Paulo antes de rodar);
    - unidades, localizações e coleções → os cadastros do item 19 e 20, com o código antigo na tabela da borda;
    - `extra` → declarações.
32. A conversão proposta para os tipos da biblioteca de Paulo, a confirmar na etapa 4:

    | Tipo | Classe e conteúdo | Suporte | Gênero ou público |
    |---|---|---|---|
    | BK Livros | `bf:Text` | `bf:Print`, volume | |
    | RO Romances | `bf:Text` | `bf:Print`, volume | gênero Romance |
    | GB Gibis | `bf:Text` + `bf:StillImage` | `bf:Print`, volume | gênero HQ |
    | LI Livros infantis | `bf:Text` | `bf:Print`, volume | público infantil |
    | JV Livros juvenis | `bf:Text` | `bf:Print`, volume | público juvenil |
    | LB Livros em braille | `bf:Text`, texto tátil | `bf:Tactile`, volume | |
    | AL Audiolivros | `bf:Audio`, palavra falada | disco de áudio ou online | |
    | DV DVDs | `bf:MovingImage` | videodisco | |

    Quando o líder e o `008`/`007` do registro dizem outra coisa, vale o registro.

## 2. Origem no Koha

- Tipo de item: `itemtypes` e `items.itype`/`biblioitems.itemtype`. É uma tabela livre que cada biblioteca preenche; serve à circulação (`circulation_rules`) e acaba usada como tipo de material.
- Localização e coleção: valores autorizados `LOC` e `CCODE`, também livres.
- Unidades: `branches`.
- O Koha não tem BIBFRAME. A referência da conversão é a da LC, não a do Koha.

## 3. Paridade

`livre` para o modelo e a interface. `exigida` para a exportação MARC: nenhum registro pode sair diferente do que sai hoje (item 30), o que os cenários de ida e volta da tarefa 1.11 já conferem.

## 4. Pontos em aberto, com a escolha feita

1. **Versão.** A diretriz fala em "BIBFRAME 2.0"; o endereço citado serve hoje a 3.0.1, que é a continuação da mesma ontologia e tem as classes que a diretriz lista. Fixamos a **3.0.1**.
2. **Coleção.** A ontologia não tem uma propriedade "coleção" para o exemplar. As coleções atuais (Ficção, Não ficção, Referência) são, em BIBFRAME, gênero da Obra (as duas primeiras) e política de uso (a terceira). A lista de coleções por unidade pedida na diretriz é mantida e sai no RDF como um segundo `bf:sublocation`; a migração também grava o gênero e a política correspondentes.
3. **Vocabulários.** `id.loc.gov` não está liberado no ambiente em que os PRs são escritos; a ontologia veio do repositório oficial da LC no GitHub. Para copiar as listas do item 11 é preciso liberar esse endereço ou Paulo baixar os arquivos.

## 5. Etapas (um PR cada, nesta ordem)

| Etapa | Entrega | Critério |
|---|---|---|
| 0 | Esta spec, o ADR 0003 e os avisos nos documentos substituídos | Paulo aprova |
| 1 | Ontologia no repositório, lista gerada, testes de cobertura ligados (com a lista do que falta registrada, que só pode diminuir) | `codegen --check` verde |
| 2 | Vocabulários da LC no repositório e nos termos | toda propriedade categórica tem a lista oficial |
| 3 | Modelo: classes, `bf:content`/`media`/`carrier`/`issuance`, tabela de declarações, fim de `extra` e de `material_type` | cobertura do modelo sem faltas |
| 4 | Unidades como organização, localizações e coleções por unidade, fim do tipo de item, política por `bf:usageAndAccessPolicy`, tabela da borda, migração | exportação MARC idêntica à de antes |
| 5 | Conversão MARC21 → BIBFRAME ampliada | ida e volta dos registros do oráculo |
| 6 | Formulários completos, sem expandir | cobertura do formulário sem faltas |
| 7 | Caixas de seleção e ícones em formulário, grade, pílulas e OPAC | toda categoria da tabela do item 15 com ícone |
| 8 | Exportação RDF/XML e Turtle (2.11) | cobertura da exportação sem faltas |

As tarefas 2.9 (autoridades) e 2.10 (listas de trabalho e facetas) vêm depois da etapa 8.

## 6. Fora deste plano

- Triple store ou SPARQL (doc 04, decisão 5).
- BIBFRAME como formato de **importação** (só MARC entra, por ora).
- UNIMARC (Fase 4).
