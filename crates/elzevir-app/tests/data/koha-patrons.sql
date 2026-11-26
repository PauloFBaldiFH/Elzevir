/*M!999999\- enable the sandbox mode */ 
-- MariaDB dump 10.19  Distrib 10.11.13-MariaDB, for debian-linux-gnu (x86_64)
--
-- Host: localhost    Database: koha_synthetic
-- ------------------------------------------------------
-- Patron 1's password is `Senha fictícia 1`; patron 6's, an old MD5 one, is not migrated.
-- A synthetic dump in the shape `koha-dump` writes (MariaDB 11), with fictitious patrons.

/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;
/*!40101 SET NAMES utf8mb4 */;
/*!40103 SET TIME_ZONE='+00:00' */;
/*!40014 SET @OLD_UNIQUE_CHECKS=@@UNIQUE_CHECKS, UNIQUE_CHECKS=0 */;

--
-- Current Database: `koha_synthetic`
--

CREATE DATABASE /*!32312 IF NOT EXISTS*/ `koha_synthetic` /*!40100 DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci */;

USE `koha_synthetic`;

DROP TABLE IF EXISTS `branches`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8mb4 */;
CREATE TABLE `branches` (
  `branchcode` varchar(10) NOT NULL DEFAULT '' COMMENT 'a unique key assigned to each branch',
  `branchname` longtext NOT NULL COMMENT 'the name of your library or branch',
  `branchaddress1` longtext DEFAULT NULL,
  `branchaddress2` longtext DEFAULT NULL,
  `branchaddress3` longtext DEFAULT NULL,
  `branchzip` varchar(25) DEFAULT NULL,
  `branchcity` longtext DEFAULT NULL,
  `branchstate` longtext DEFAULT NULL,
  `branchcountry` mediumtext DEFAULT NULL,
  `branchphone` longtext DEFAULT NULL,
  `branchfax` longtext DEFAULT NULL,
  `branchemail` longtext DEFAULT NULL,
  `branchurl` longtext DEFAULT NULL COMMENT 'the URL for your library or branch''s website',
  `pickup_location` tinyint(1) NOT NULL DEFAULT 1,
  PRIMARY KEY (`branchcode`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

LOCK TABLES `branches` WRITE;
/*!40000 ALTER TABLE `branches` DISABLE KEYS */;
INSERT INTO `branches` VALUES ('CTR','Biblioteca Central','Rua das Flores, 100',NULL,'','85950-000','Cidade Exemplo','PR','Brasil','(44) 3000-0000',NULL,'central@example.org','https://biblioteca.example.org',1),('NRT','Ramal Norte',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,1);
/*!40000 ALTER TABLE `branches` ENABLE KEYS */;
UNLOCK TABLES;

DROP TABLE IF EXISTS `categories`;
CREATE TABLE `categories` (
  `categorycode` varchar(10) NOT NULL DEFAULT '',
  `description` longtext DEFAULT NULL,
  `enrolmentperiod` smallint(6) DEFAULT NULL,
  `enrolmentperioddate` date DEFAULT NULL,
  `enrolmentfee` decimal(28,6) DEFAULT NULL,
  `category_type` varchar(1) NOT NULL DEFAULT 'A',
  `default_privacy` enum('default','never','forever') NOT NULL DEFAULT 'default',
  PRIMARY KEY (`categorycode`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

LOCK TABLES `categories` WRITE;
INSERT INTO `categories` VALUES ('ADU','Adulto',12,NULL,0.000000,'A','default'),('EST','Estudante',NULL,'2027-12-31',NULL,'C','never'),('INS',NULL,NULL,NULL,NULL,'I','default');
UNLOCK TABLES;

DROP TABLE IF EXISTS `cover_images`;
CREATE TABLE `cover_images` (
  `imagenumber` int(11) NOT NULL AUTO_INCREMENT,
  `mimetype` varchar(15) NOT NULL,
  `imagefile` mediumblob NOT NULL,
  `ratio` float DEFAULT NULL,
  `timestamp` timestamp NOT NULL DEFAULT current_timestamp() ON UPDATE current_timestamp(),
  PRIMARY KEY (`imagenumber`),
  KEY `bibnum` (`imagenumber`),
  CONSTRAINT `check_ratio` CHECK (`ratio` > 0)
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci COMMENT='images, with a quote''s comment';

LOCK TABLES `cover_images` WRITE;
INSERT INTO `cover_images` VALUES (1,'image/png',_binary '\0\Z\n\r\\\'',1.5e-05,'2024-01-01 00:00:00'),(2,'image/gif',0x47494638,-2.25E+3,'2024-01-02 00:00:00'),(3,'image/gif',X'4749',0,'2024-01-03 00:00:00');
UNLOCK TABLES;

DROP TABLE IF EXISTS `borrowers`;
CREATE TABLE `borrowers` (
  `borrowernumber` int(11) NOT NULL AUTO_INCREMENT COMMENT 'primary key',
  `cardnumber` varchar(32) DEFAULT NULL,
  `surname` longtext DEFAULT NULL,
  `firstname` mediumtext DEFAULT NULL,
  `title` longtext DEFAULT NULL,
  `streetnumber` tinytext DEFAULT NULL,
  `streettype` tinytext DEFAULT NULL,
  `address` longtext DEFAULT NULL,
  `address2` mediumtext DEFAULT NULL,
  `city` longtext DEFAULT NULL,
  `state` mediumtext DEFAULT NULL,
  `zipcode` tinytext DEFAULT NULL,
  `country` mediumtext DEFAULT NULL,
  `email` longtext DEFAULT NULL,
  `phone` mediumtext DEFAULT NULL,
  `mobile` tinytext DEFAULT NULL,
  `emailpro` mediumtext DEFAULT NULL,
  `phonepro` mediumtext DEFAULT NULL,
  `B_city` longtext DEFAULT NULL,
  `dateofbirth` date DEFAULT NULL,
  `branchcode` varchar(10) NOT NULL DEFAULT '',
  `categorycode` varchar(10) NOT NULL DEFAULT '',
  `dateenrolled` date DEFAULT NULL,
  `dateexpiry` date DEFAULT NULL,
  `gonenoaddress` tinyint(1) DEFAULT NULL,
  `borrowernotes` longtext DEFAULT NULL,
  `sex` varchar(1) DEFAULT NULL,
  `password` varchar(60) DEFAULT NULL,
  `flags` bigint(11) DEFAULT NULL,
  `userid` varchar(75) DEFAULT NULL,
  `privacy` int(11) NOT NULL DEFAULT 1,
  `updated_on` timestamp NOT NULL DEFAULT current_timestamp() ON UPDATE current_timestamp(),
  PRIMARY KEY (`borrowernumber`),
  UNIQUE KEY `cardnumber` (`cardnumber`),
  KEY `categorycode` (`categorycode`),
  CONSTRAINT `borrowers_ibfk_1` FOREIGN KEY (`categorycode`) REFERENCES `categories` (`categorycode`)
) ENGINE=InnoDB AUTO_INCREMENT=7 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

SET @OLD_AUTOCOMMIT=@@AUTOCOMMIT, @@AUTOCOMMIT=0;
LOCK TABLES `borrowers` WRITE;
/*!40000 ALTER TABLE `borrowers` DISABLE KEYS */;
INSERT INTO `borrowers` VALUES (1,'A0001','Silva','Ana Maria','Sra.','100','Rua','das Acácias','apto 2','Cidade Exemplo','PR','85950-000','Brasil','ana@example.org','(44) 99999-0001',NULL,NULL,NULL,'Outra Cidade','1990-05-10','CTR','ADU','2020-01-15','2030-01-15',0,'Leitora fictícia','F','$2a$08$KBCwKxOzLha2MUDgW0PjXeL8PLapJrHH7Qyn8x/oWtvIYu16Gx/s2',NULL,'ana',0,'2024-01-01 00:00:00'),(2,'A0002',NULL,'Escola Municipal Exemplo',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'sem-arroba',NULL,'(44) 3000-0002',NULL,NULL,NULL,'0000-00-00','NRT','INS',NULL,NULL,1,NULL,NULL,NULL,1,'escola',1,'2024-01-01 00:00:00'),(3,NULL,'Sem Carteirinha','Bruno',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'CTR','ADU','2021-03-01','2031-03-01',NULL,NULL,NULL,NULL,NULL,NULL,1,'2024-01-01 00:00:00'),(4,'A0004','Souza','Carla',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'XXX','ADU','2021-03-01','2031-03-01',NULL,NULL,NULL,NULL,NULL,NULL,1,'2024-01-01 00:00:00'),(5,'A0005','Lima','Davi',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'CTR','ZZ','2021-03-01','2031-03-01',NULL,NULL,NULL,NULL,NULL,NULL,2,'2024-01-01 00:00:00'),(6,'A0006','Pereira','Elisa',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'elisa@example.org',NULL,NULL,'2025-06-01','CTR','EST','2025-01-10','2024-12-31',NULL,NULL,NULL,'X03MO1qnZdYdgyfeuILPmQ',NULL,NULL,2,'2024-01-01 00:00:00');
/*!40000 ALTER TABLE `borrowers` ENABLE KEYS */;
UNLOCK TABLES;
COMMIT;
SET AUTOCOMMIT=@OLD_AUTOCOMMIT;

DROP TABLE IF EXISTS `borrower_debarments`;
CREATE TABLE `borrower_debarments` (
  `borrower_debarment_id` int(11) NOT NULL AUTO_INCREMENT,
  `borrowernumber` int(11) NOT NULL,
  `expiration` date DEFAULT NULL,
  `type` varchar(50) NOT NULL,
  `comment` mediumtext DEFAULT NULL,
  `manager_id` int(11) DEFAULT NULL,
  `created` timestamp NOT NULL DEFAULT current_timestamp() ON UPDATE current_timestamp(),
  `updated` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`borrower_debarment_id`)
) ENGINE=InnoDB AUTO_INCREMENT=6 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

LOCK TABLES `borrower_debarments` WRITE;
INSERT INTO `borrower_debarments` VALUES (1,1,'2099-01-01','SUSPENSION','Atraso',NULL,'2024-03-01 10:00:00',NULL),(2,1,'9999-12-31','MANUAL','Bloqueio',NULL,'2024-02-01 09:30:00',NULL),(3,1,NULL,'OVERDUES','Aviso',NULL,'2024-04-01 08:00:00',NULL),(4,6,'2000-01-01','SUSPENSION',NULL,NULL,'1999-12-01 08:00:00',NULL),(5,3,NULL,'MANUAL',NULL,NULL,'2024-01-01 08:00:00',NULL);
UNLOCK TABLES;
DROP TABLE IF EXISTS `items`;
CREATE TABLE `items` (
  `itemnumber` int(11) NOT NULL AUTO_INCREMENT,
  `biblionumber` int(11) NOT NULL DEFAULT 0,
  `barcode` varchar(20) DEFAULT NULL,
  `homebranch` varchar(10) DEFAULT NULL,
  PRIMARY KEY (`itemnumber`)
) ENGINE=InnoDB AUTO_INCREMENT=121 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

LOCK TABLES `items` WRITE;
INSERT INTO `items` VALUES (100,10,'B100','CTR'),(101,10,'B101','CTR'),(110,11,'B110','CTR'),(120,11,'B120','CTR');
UNLOCK TABLES;

DROP TABLE IF EXISTS `issues`;
CREATE TABLE `issues` (
  `issue_id` int(11) NOT NULL AUTO_INCREMENT,
  `borrowernumber` int(11) DEFAULT NULL,
  `issuer_id` int(11) DEFAULT NULL,
  `itemnumber` int(11) DEFAULT NULL,
  `date_due` datetime DEFAULT NULL,
  `branchcode` varchar(10) DEFAULT NULL,
  `returndate` datetime DEFAULT NULL,
  `lastreneweddate` datetime DEFAULT NULL,
  `renewals_count` tinyint(4) NOT NULL DEFAULT 0,
  `unseen_renewals` tinyint(4) NOT NULL DEFAULT 0,
  `auto_renew` tinyint(1) DEFAULT 0,
  `auto_renew_error` varchar(32) DEFAULT NULL,
  `timestamp` timestamp NOT NULL DEFAULT current_timestamp() ON UPDATE current_timestamp(),
  `issuedate` datetime DEFAULT NULL,
  `onsite_checkout` int(1) NOT NULL DEFAULT 0,
  `note` longtext DEFAULT NULL,
  `notedate` datetime DEFAULT NULL,
  `noteseen` int(1) DEFAULT NULL,
  PRIMARY KEY (`issue_id`)
) ENGINE=InnoDB AUTO_INCREMENT=7 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

LOCK TABLES `issues` WRITE;
INSERT INTO `issues` VALUES (1,1,NULL,100,'2026-12-10 23:59:00','CTR',NULL,'2026-11-20 10:00:00',1,0,0,NULL,'2026-11-20 10:00:00','2026-11-05 14:30:00',0,NULL,NULL,NULL),(2,6,NULL,120,'2026-11-27 18:00:00','NRT',NULL,NULL,0,0,0,NULL,'2026-11-26 09:00:00','2026-11-26 09:00:00',1,NULL,NULL,NULL),(3,3,NULL,101,'2026-12-10 23:59:00','CTR',NULL,NULL,0,0,0,NULL,'2026-11-05 14:30:00','2026-11-05 14:30:00',0,NULL,NULL,NULL),(4,1,NULL,999,'2026-12-10 23:59:00','CTR',NULL,NULL,0,0,0,NULL,'2026-11-05 14:30:00','2026-11-05 14:30:00',0,NULL,NULL,NULL),(5,2,NULL,100,'2026-12-10 23:59:00','CTR',NULL,NULL,0,0,0,NULL,'2026-11-05 14:30:00','2026-11-05 14:30:00',0,NULL,NULL,NULL),(6,2,NULL,110,'2026-12-10 23:59:00','CTR',NULL,NULL,0,0,0,NULL,'2026-11-05 14:30:00',NULL,0,NULL,NULL,NULL);
UNLOCK TABLES;

DROP TABLE IF EXISTS `reserves`;
CREATE TABLE `reserves` (
  `reserve_id` int(11) NOT NULL AUTO_INCREMENT,
  `borrowernumber` int(11) NOT NULL DEFAULT 0,
  `reservedate` date DEFAULT NULL,
  `biblionumber` int(11) NOT NULL DEFAULT 0,
  `deleted_biblionumber` int(11) DEFAULT NULL,
  `item_group_id` int(11) DEFAULT NULL,
  `branchcode` varchar(10) NOT NULL,
  `desk_id` int(11) DEFAULT NULL,
  `notificationdate` date DEFAULT NULL,
  `reminderdate` date DEFAULT NULL,
  `cancellationdate` date DEFAULT NULL,
  `cancellation_reason` varchar(80) DEFAULT NULL,
  `reservenotes` longtext DEFAULT NULL,
  `priority` smallint(6) NOT NULL DEFAULT 1,
  `found` varchar(1) DEFAULT NULL,
  `timestamp` timestamp NOT NULL DEFAULT current_timestamp() ON UPDATE current_timestamp(),
  `itemnumber` int(11) DEFAULT NULL,
  `waitingdate` date DEFAULT NULL,
  `expirationdate` date DEFAULT NULL,
  `patron_expiration_date` date DEFAULT NULL,
  `lowestPriority` tinyint(1) NOT NULL DEFAULT 0,
  `suspend` tinyint(1) NOT NULL DEFAULT 0,
  `suspend_until` datetime DEFAULT NULL,
  `itemtype` varchar(10) DEFAULT NULL,
  `item_level_hold` tinyint(1) NOT NULL DEFAULT 0,
  `non_priority` tinyint(1) NOT NULL DEFAULT 0,
  PRIMARY KEY (`reserve_id`)
) ENGINE=InnoDB AUTO_INCREMENT=7 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

LOCK TABLES `reserves` WRITE;
INSERT INTO `reserves` VALUES (1,2,'2026-11-10',10,NULL,NULL,'CTR',NULL,NULL,NULL,NULL,NULL,NULL,1,NULL,'2026-11-10 10:00:00',NULL,NULL,NULL,NULL,0,0,NULL,NULL,0,0),(2,6,'2026-11-12',10,NULL,NULL,'NRT',NULL,NULL,NULL,NULL,NULL,NULL,2,NULL,'2026-11-12 10:00:00',NULL,NULL,'2027-01-31',NULL,0,1,NULL,NULL,0,0),(3,1,'2026-11-01',11,NULL,NULL,'CTR',NULL,NULL,NULL,NULL,NULL,NULL,0,'W','2026-11-24 10:00:00',110,'2026-11-24','2026-12-01',NULL,0,0,NULL,NULL,0,0),(4,1,'2026-11-03',10,NULL,NULL,'NRT',NULL,NULL,NULL,NULL,NULL,NULL,0,'T','2026-11-25 10:00:00',101,NULL,NULL,NULL,0,0,NULL,NULL,1,0),(5,2,'2026-11-15',99,NULL,NULL,'CTR',NULL,NULL,NULL,NULL,NULL,NULL,1,NULL,'2026-11-15 10:00:00',NULL,NULL,NULL,NULL,0,0,NULL,NULL,0,0),(6,6,'2026-11-16',11,NULL,NULL,'ZZZ',NULL,NULL,NULL,NULL,NULL,NULL,1,NULL,'2026-11-16 10:00:00',NULL,NULL,NULL,NULL,0,0,NULL,NULL,0,0);
UNLOCK TABLES;
DROP TABLE IF EXISTS `accountlines`;
CREATE TABLE `accountlines` (
  `accountlines_id` int(11) NOT NULL AUTO_INCREMENT,
  `issue_id` int(11) DEFAULT NULL,
  `old_issue_id` int(11) DEFAULT NULL,
  `borrowernumber` int(11) DEFAULT NULL,
  `itemnumber` int(11) DEFAULT NULL,
  `date` timestamp NULL DEFAULT NULL,
  `amount` decimal(28,6) DEFAULT NULL,
  `description` longtext DEFAULT NULL,
  `credit_type_code` varchar(80) DEFAULT NULL,
  `debit_type_code` varchar(80) DEFAULT NULL,
  `credit_number` varchar(20) DEFAULT NULL,
  `status` varchar(16) DEFAULT NULL,
  `payment_type` varchar(80) DEFAULT NULL,
  `amountoutstanding` decimal(28,6) DEFAULT NULL,
  `timestamp` timestamp NOT NULL DEFAULT current_timestamp() ON UPDATE current_timestamp(),
  `note` mediumtext DEFAULT NULL,
  `manager_id` int(11) DEFAULT NULL,
  `register_id` int(11) DEFAULT NULL,
  `interface` varchar(16) NOT NULL,
  `branchcode` varchar(10) DEFAULT NULL,
  PRIMARY KEY (`accountlines_id`)
) ENGINE=InnoDB AUTO_INCREMENT=8 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

LOCK TABLES `accountlines` WRITE;
INSERT INTO `accountlines` VALUES (1,1,NULL,1,100,'2026-11-25 23:59:00','2.000000',NULL,NULL,'OVERDUE',NULL,'UNRETURNED',NULL,'2.000000','2026-11-25 23:59:00',NULL,NULL,NULL,'intranet','CTR'),(2,NULL,NULL,1,NULL,'2026-11-26 10:00:00','-1.000000',NULL,'PAYMENT',NULL,NULL,NULL,NULL,'0.000000','2026-11-26 10:00:00',NULL,NULL,NULL,'intranet','CTR'),(3,NULL,NULL,1,101,'2026-10-01 12:00:00','3.000000',NULL,NULL,'OVERDUE',NULL,'RETURNED',NULL,'2.000000','2026-11-26 10:00:00',NULL,NULL,NULL,'intranet','CTR'),(4,NULL,NULL,6,NULL,NULL,'0.333333',NULL,NULL,'MULTA_LOCAL',NULL,NULL,NULL,'0.333333','2026-11-20 09:00:00',NULL,NULL,NULL,'intranet','NRT'),(5,NULL,NULL,3,NULL,'2026-11-20 09:00:00','5.000000',NULL,NULL,'MANUAL',NULL,NULL,NULL,'5.000000','2026-11-20 09:00:00',NULL,NULL,NULL,'intranet','CTR'),(6,NULL,NULL,2,NULL,'2026-11-20 09:00:00','5.000000',NULL,NULL,NULL,NULL,NULL,NULL,'5.000000','2026-11-20 09:00:00',NULL,NULL,NULL,'intranet','CTR'),(7,NULL,NULL,6,NULL,'2026-11-21 09:00:00','-1.000000',NULL,'DOACAO',NULL,NULL,NULL,NULL,'-1.000000','2026-11-21 09:00:00',NULL,NULL,NULL,'intranet','NRT');
UNLOCK TABLES;

DROP TABLE IF EXISTS `account_offsets`;
CREATE TABLE `account_offsets` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `credit_id` int(11) DEFAULT NULL,
  `debit_id` int(11) DEFAULT NULL,
  `type` enum('CREATE','APPLY','VOID','OVERDUE_INCREASE','OVERDUE_DECREASE') NOT NULL,
  `amount` decimal(26,6) NOT NULL,
  `created_on` timestamp NOT NULL DEFAULT current_timestamp(),
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=7 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

LOCK TABLES `account_offsets` WRITE;
INSERT INTO `account_offsets` VALUES (1,NULL,1,'CREATE',2.000000,'2026-11-25 23:59:00'),(2,2,NULL,'CREATE',-1.000000,'2026-11-26 10:00:00'),(3,2,3,'APPLY',-1.000000,'2026-11-26 10:00:00'),(4,NULL,3,'CREATE',3.000000,'2026-10-01 12:00:00'),(5,NULL,99,'CREATE',1.000000,'2026-01-01 08:00:00'),(6,NULL,5,'CREATE',5.000000,'2026-11-20 09:00:00');
UNLOCK TABLES;
/*!40101 SET SQL_MODE=@OLD_SQL_MODE */;

-- Dump completed on 2026-11-26 12:00:00
