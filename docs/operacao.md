# Operação do Elzevir

O servidor, o painel e os comandos administrativos pertencem ao mesmo
executável. Cada instituição tem configuração, dados, trava e canal local
próprios. O catálogo continua funcionando quando o painel é fechado.

## Instalação gerenciada

Linux x86_64/ARM64 exige systemd ou OpenRC para registrar o serviço. Windows
x64/ARM64 exige Windows 10 1809+ ou Server 2019+. O banco deve ficar em disco
local. Para instalar no PATH do usuário e rodar pelo terminal, siga a
[instalação rápida](../README.md#instalação-rápida).

Com os scripts e uma release disponíveis:

```sh
sudo bash install.sh --name "Minha biblioteca" --dry-run
sudo bash install.sh --name "Minha biblioteca"
```

No PowerShell elevado:

```powershell
.\install.ps1 -Name 'Minha biblioteca' -Plan
.\install.ps1 -Name 'Minha biblioteca'
```

A senha do administrador é pedida pelo próprio Elzevir. Não a coloque em
argumentos. Os instaladores verificam a release antes de executar o binário,
criam conta limitada para o serviço e preservam instalações existentes.
A primeira criação verifica se a porta está disponível. Firewall é opção
explícita. O instalador não cria exclusão do Defender; o atalho do painel no
menu Iniciar precisa ser aberto com "Executar como administrador" para
controlar o serviço. Reexecutar o instalador usa o fluxo de
atualização com backup; não recria o banco ou o administrador.

Na instituição padrão, Linux usa /etc/elzevir/library.toml e
/var/lib/elzevir/library; Windows usa ProgramData\Elzevir\library.
Instituições adicionais usam --id/-Id, diretórios e executáveis separados.
Escolha portas diferentes para serviços que compartilham a máquina. O plano
mostra os caminhos finais sem baixar nem instalar.

## Painel e estado

```sh
elzevir panel --demo --lang pt-BR
elzevir panel --config CAMINHO/server.toml --lang pt-BR
elzevir status --config CAMINHO/server.toml --json
```

Ao abrir `elzevir` em uma instalação pessoal nova, use **Ajustes → Configurar biblioteca** quando estiver pronto. Ao terminar, o painel volta à biblioteca criada. **Serviço → Iniciar** coloca o catálogo no ar. Se o painel indicar uma configuração inválida ou dados existentes, confira o caminho exibido. Em um banco legado parado, a inspeção tenta identificar se já há administrador.

Em **Backup → Transferir**, informe o endereço ou alias que usa para entrar no servidor por SSH. Clique em **Copiar comando** e execute-o em uma pasta vazia no seu computador, usando Bash ou PowerShell com OpenSSH 9 ou posterior. A cópia é o arquivo cifrado da biblioteca. Guarde a chave de recuperação separadamente. Em um desktop local, **Abrir pasta** leva ao diretório de backups.

O idioma padrão é inglês; --lang pt-BR seleciona português. --ascii permite
bordas simples em terminais sem os caracteres usados pelo painel.

O painel separa configurações de observações e de testes públicos. A ajuda
de cada seção mostra suas teclas. Operações de escrita têm confirmação;
Esc cancela o formulário antes do envio. Restaurar, atualizar e controlar o
serviço exigem o operador elevado. Tarefas aceitas continuam fora da UI.
O histórico mostra falhas/interrupções, sem conteúdo de senhas ou tokens.

## Escolha do fluxo de importação

Para preparar registros de catálogo, use o importador da equipe ou
`elzevir import stage`, confira a prévia e confirme a revisão. O perfil
Biblivre 949 exige a interpretação dos campos locais e a seleção dos
cadastros da instituição. Uma migração operacional de instalação Koha
tem comandos, pré-requisitos e verificação próprios. A seleção combinada
de acervo, leitores e autoridades está em estudo. Consulte a compatibilidade
por classe antes de oferecer uma migração à biblioteca.
[Importação de catálogo](specs/catalogo/importacao-universal.md) ·
[Troca do Koha](specs/migracao/troca-do-koha.md) ·
[Inventário e limites da v4](evidencias/importacao-v4/README.md)

Para conferir um dump PostgreSQL textual antes da migração, execute
`elzevir import inventory "arquivo.backup"` ou abra **Inventariar tabelas
PostgreSQL** na área de importação da equipe. A tabela mostra linhas, nulos
e vazios por tabela de origem. Reenvie o arquivo para atualizar o inventário.
A conta da equipe precisa de catalogação, leitores e circulação.

Para conferir um grupo de arquivos da mesma instalação, informe uma origem
estável, selecione os arquivos e clique em **Preparar conjunto de fontes**.
Para extrair arquivos de um ZIP ou gzip, envie o contêiner primeiro e abra
seu inventário para escolher os membros. **Conferir integridade das fontes**
relê os bytes privados e verifica cada SHA-256. Cancelar o conjunto remove
as cópias selecionadas. Se o contêiner também foi preparado, cancele seu
rascunho separadamente quando terminar. O conjunto serve para conferência
antes da migração. [Comandos da CLI e limites](specs/catalogo/importacao-universal-v4.md#conjunto-privado-de-fontes-1b)

## Backups e recuperação

```sh
elzevir backup now --config CAMINHO/server.toml
elzevir operations --config CAMINHO/server.toml list
elzevir backup key --data-dir CAMINHO/data --output DESTINO-NOVO/backup.key
```

A cópia completa contém uma imagem SQLite consistente, configurações e segredos
cifrados com age. A chave de recuperação fica fora do arquivo. Exporte-a
explicitamente para um destino privado separado: o arquivo cifrado sozinho
não permite restauração. Índice, logs e certificado mestre Cloudflare não
entram no backup. O índice é reconstruído ao restaurar.

A seção Backup permite criar, verificar, exportar, restaurar e definir
retenção/agenda local e de nuvem. No painel, a restauração gerenciada para o
serviço, preserva os arquivos anteriores e verifica saúde após reiniciar.
Em **Backup**, pressione `o` e use `F2` no campo da cópia ou da chave.
`F4` lista as cópias gerenciadas da instituição. `F3` abre a pasta pessoal
no host do painel. Use `F5` para incluir arquivos com outras extensões.
Em SSH, os dois caminhos devem existir no servidor. Colar um caminho só
preenche o campo. F7 escolhe a forma literal e F8 a interpretada quando
ambas identificam arquivos. **Validar cópia** autentica o pacote e verifica
o banco antes de abrir a revisão. O foco começa em **Voltar**. Para aplicar,
selecione **Restaurar** com a seta e confirme com Enter.
**Somente banco** preserva as configurações e credenciais da instalação e
reconstrói o índice. O histórico distingue falha antes da troca, retorno à
instalação anterior e recuperação pendente.
Falha de saúde provoca tentativa de retorno à instalação anterior. O comando
backup restore é offline e exige servidor parado e --confirm-restore.

Se uma troca de arquivos for interrompida, o servidor recusa abrir um diário
pendente. Com a instituição parada, a recuperação explícita é:

```sh
elzevir backup recover-restore --data-dir CAMINHO/data --config CAMINHO/server.toml --confirm-restore
```

## Nuvem e endereços públicos

Os instaladores não instalam ferramentas opcionais nem autorizam contas.
No painel, instale rclone para conexões/planos e cloudflared para túneis.
A configuração rclone roda na conta limitada do servidor; mantenha o serviço
em execução para responder aos formulários pelo canal administrativo local.
Transferências usam nomes próprios e retenção de artefatos reconhecidos, sem
sync/purge sobre o remoto. Falha de envio preserva a cópia local.

Em **Backup**, `[` e `]` alternam entre **Conexões**, **Planos**, **Na nuvem**
e **Histórico**. Use `<` e `>` para selecionar o plano. A confirmação mostra
a conexão e a pasta. Em **Planos**, `u` cria uma cópia nova e `U` envia ou
confere uma cópia local por ID. Para usar o mesmo arquivo em vários planos,
marque cada destino com `+` e pressione `M`. PgUp/PgDn percorrem os destinos na confirmação.

**Envio a confirmar** registra uma transferência cujo resultado ainda precisa
ser conferido. Use `U` com o mesmo ID. O serviço compara o SHA-256 remoto
e mantém a cópia local disponível para nova tentativa. A retenção só remove
objetos registrados e com conteúdo conferido. Registros antigos sem SHA-256
permanecem no remoto.

Em **Na nuvem**, use `l` para consultar uma página de até 20 arquivos, `,` e
`.` para selecionar e `D` para baixar. A consulta admite até 8.192 entradas e
2 MiB de metadados por pasta. O painel mostra **Indisponível** se o provedor
omitir a quota. O arquivo baixado fica em `DATA_DIR/cloud-downloads` após
verificação de tamanho e SHA-256. Use `O` para informar a chave e iniciar
**Validar cópia**. O serviço continua ativo até a confirmação de restauração.

Domínio é opcional e pode ser escolhido posteriormente, sem recompilar.
Domínio próprio requer autorização da conta Cloudflare e dois hostnames
distintos para catálogo/equipe. Para endereço gratuito, abra **Endereços e acesso**
e pressione **g** no fluxo **Domínio & Túnel (*.elzevir.app)**. O padrão é
https://broker.elzevir.app; deixe o campo de URL vazio para usar esse padrão ou
ELZEVIR_BROKER_URL. Informe biblioteca, contato e slug opcional. Confirme o
cadastro e, se houver revisão pendente, use o link de autorização apresentado.
O painel retoma o provisionamento e mostra os endereços atribuídos e o estado
local do cloudflared. Use **c** para atualizar o estado remoto, **w/W** para
configurar/desativar acesso da equipe, **n** para rotacionar o token e **z**
para desvincular. Testes HTTPS continuam disponíveis em **t**.

A CLI oferece o mesmo fluxo (terminal de administrador/operador):

```sh
elzevir address --config /caminho/server.toml --confirm --wait enroll --institution 'Fictional library' --email contato@example.org --name minha-biblioteca
elzevir address --config /caminho/server.toml status
elzevir address --config /caminho/server.toml --wait refresh
elzevir address --config /caminho/server.toml --confirm --wait staff --username equipe --cidr 192.0.2.0/24
elzevir address --config /caminho/server.toml --confirm --wait renew
```

A inscrição aceita `--broker` ou `--broker-url` para override da URL HTTPS.
A senha remota é lida de forma mascarada, sem argumento de linha de comando.
A identidade Ed25519 fica no arquivo privado `DATA_DIR/secrets/broker.json`.
Instalar/iniciar o programa não registra domínio nem DNS automaticamente;
esses efeitos só ocorrem após o cadastro confirmado pelo operador. O contrato
é validado com respostas simuladas; a validação pública real exige executar
o fluxo contra o serviço e testar DNS/HTTPS.

## Atualizações

A seção Atualizações consulta releases e atualiza Elzevir e ferramentas. O
Elzevir fixa a versão, verifica o pacote, faz backup, para o serviço e verifica
a nova saúde. O executável e o banco anteriores são preservados para rollback.
Atualização de rclone recusa ferramenta em uso. Atualizar cloudflared mantém a
intenção anterior do conector selecionado, sem parar outras instituições.

Após interrupção de atualização, use r na seção Atualizações ou outro binário
Elzevir disponível para revisar/recuperar o executável instalado:

```sh
elzevir recover-update --executable CAMINHO/elzevir --dry-run
elzevir recover-update --executable CAMINHO/elzevir --confirm-recovery --wait
```

O diário privado identifica a instituição. A recuperação recusa outro config
ou arquivos divergentes. Uma versão nova já saudável apenas conclui o diário,
preservando suas escritas. Uma recuperação incompleta mantém os arquivos de
segurança para nova tentativa.

## Situação do roadmap operacional

| Etapas | Código entregue | Validação externa posterior |
|---|---|---|
| A1–A4 | Configuração, init, saúde/logs/status e IPC local | Uso na instalação real |
| A5–A6 | Backup cifrado, verificação, restauração, retenção e agendas | Recuperação com cópias reais do operador |
| A7 | systemd, OpenRC e SCM Windows, parada com checkpoint | Registro/início em cada sistema nativo |
| A8–A9 | Workflow de quatro alvos, checksums e instaladores | Executar/publicar uma release validada |
| P1–P4 | Painel, observações, diagnóstico e modos de acesso | Avaliação de uso |
| P5 | Túnel de domínio próprio e teste público | Conta Cloudflare, DNS e domínio escolhido |
| P6–P7 | Device Flow do kei-broker, domínio gratuito elzevir.app, equipe remota, recuperação e QR | Validar inscrição, DNS/HTTPS e túnel no serviço público |
| P8–P10 | Central de backup, rclone, crypt e planos de nuvem | Autorizar/testar provedores escolhidos |
| P11 | Atualizações e recuperação do Elzevir e das ferramentas | Pacotes oficiais e execução nativa |
| P12 | Contagem de sessões, reinício de serviço e máquina | Controle nativo na máquina do operador |

Testes locais usam instituições fictícias e adaptadores simulados para
serviços, downloads e nuvem. Compilação de adaptadores para Windows verifica
tipos/APIs, sem executar SCM, ACLs ou processos Windows. O CI possui testes
Windows e verificações do PowerShell 5.1; publicar o PR permite validá-los na
plataforma. macOS/launchd não faz parte dos quatro alvos desta distribuição.
