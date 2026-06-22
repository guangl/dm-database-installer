/*
 **************************************************************************
 * *
 * Auto parameter adjustment 5.3 for dm8*
 * [Oct. 27, 2025 ]
 * Take effect after restart dmserver
 * *
 **************************************************************************
*/
declare                  --本脚本为安全生产环境设置参数作为参考，并不追求极致性能   
exec_mode int:= 0;       --0表示脚本自动获取机器的内存和CPU配置直接执行脚本修改参数；1表示不直接修改参数，只打印设置参数的语句，设置为1后，必须调整v_mem_mb和v_cpus
                         --dm-installer 静默安装场景下固定为 0，自动按机器实际内存/CPU调整参数
is_dsc int :=  0;        --是否是dsc集群,如果是dsc集群请设置为1，将自动调整dsc相关参数
mem_per int:=  80;       --默认机器80%内存归达梦数据库使用，可根据实际需求调整此参数; MAX_OS_MEMORY强制100不与此参数挂钩
v_mem_mb int:= 64000;    --exec_mode为1时请自行根据机器实际内存调整此参数，单位为M；v_mem_mb*(mem_per/100)小于4G,将不做调整
v_cpus int:= 64;         --exec_mode为1时请自行根据机器实际CPU核数调整此参数
oltp_mode int:=0;        --并发量较高的OLTP类型系统此参数设置为1，并发量不高的一般业务系统和OLAP类的系统此参数设置为0,影响SORT_FLAG和UNDO_RETENTION 
pk_cluster_mode int:=1;  --是否使用聚集主键：性能要求高且大字段较少的业务场景强烈建议设置为1，大字段多的场景设置为0
ini_bak int:=0;          --是否建一个表备份老的dm.ini,1为保存，0为不保存，默认不保存
tname varchar(100);
MAX_SESSIONS INT :=100;  --本脚本根据内存参数自动计算能支持的最大连接数MAX_SESSIONS，如计算出的MAX_SESSIONS不满足生产要求，建议给机器加内存资源
MEMORY_POOL int;
MEMORY_N_POOLS int;
MEMORY_TARGET int;
BUFFER INT;
MAX_BUFFER INT;
RECYCLE int;
CACHE_POOL_SIZE int;
BUFFER_POOLS int;
RECYCLE_POOLS int;
SORT_BUF_SIZE int;
SORT_BUF_GLOBAL_SIZE INT;
DICT_BUF_SIZE  INT:=50;
HJ_BUF_SIZE INT;
HAGR_BUF_SIZE INT;
HJ_BUF_GLOBAL_SIZE INT;
HAGR_BUF_GLOBAL_SIZE INT;
SORT_FLAG INT;
SORT_BLK_SIZE INT;
RLOG_POOL_SIZE INT;
TASK_THREADS INT;
IO_THR_GROUPS INT;
FAST_POOL_PAGES INT :=3000;
FAST_ROLL_PAGES INT :=1000;
UNDO_RETENTION INT :=90;
VM_POOL_TARGET INT := 8192;
SESS_POOL_TARGET INT := 8192;
CNT INT;
VER0 INT :=0;
VER1 INT :=0;
VER2 INT :=0;
flag INT :=0;
begin
    CNT :=0;
    if exec_mode=0 then 
	  SELECT TOP 1 N_CPU,TOTAL_PHY_SIZE/1024/1024 INTO v_cpus,v_mem_mb FROM V$SYSTEMINFO;
	end if;
	
	if v_mem_mb>= 128000 then
	  flag:=1;
	end if;
	
	v_mem_mb := v_mem_mb * (mem_per/100.0);

	
	v_mem_mb=round(v_mem_mb,-3);
	
	--内存4G以下的不做调整，采用默认参数
	IF v_mem_mb < 4000  THEN
	  goto return_2000;
	END IF;
	
	--MEMORY_TARGET=round(cast(v_mem_mb * 0.10 as int),-3);
	MEMORY_TARGET=GREAT(FLOOR(cast(v_mem_mb * 0.10 as int)/1000)*1000,500);
	
	TASK_THREADS :=4;
	IO_THR_GROUPS :=8;
	
	
	IF v_cpus > 64 THEN 
	    v_cpus := 64; 
	    TASK_THREADS := 16;
	    IO_THR_GROUPS := 32;
	ELSIF v_cpus > 8 THEN 
	    IO_THR_GROUPS := 16;
	    TASK_THREADS := 8;
	ELSE   
	    TASK_THREADS := 4;
	    IO_THR_GROUPS := 8;
	END IF;
		
	
	--BUFFER := round(cast(v_mem_mb * 0.4 as int),-3);
	BUFFER := FLOOR(cast(v_mem_mb * 0.4 as int)/1000)*1000;
	RECYCLE :=cast(v_mem_mb * 0.04 as int);
	
	
  IF v_mem_mb < 70000 THEN
	
       with t as
        (
                select rownum rn from dual connect by level <= 100
        ) ,
        t1 as
        (
                select * from t where rn > 1 minus
                select
                        ta.rn * tb.rn
                from
                        t ta,
                        t tb
                where
                        ta.rn <= tb.rn
                    and ta.rn  > 1
                    and tb.rn  > 1
        )
      select top 1 rn into BUFFER_POOLS from t1 where rn > v_mem_mb/800 order by 1;     
        
    ELSE
       BUFFER_POOLS := 101;
    END IF;
	
	--修改内存池
	IF v_mem_mb >= 16000  THEN 
	   IF v_mem_mb= 16000 THEN
	      MEMORY_POOL := 1000;
	      SORT_BUF_GLOBAL_SIZE := 1000;
		  MEMORY_N_POOLS := 3;
		  CACHE_POOL_SIZE := 512;
          DICT_BUF_SIZE := 256;
	   ELSE
	      MEMORY_POOL := 2000;
	      SORT_BUF_GLOBAL_SIZE := 2000;
		  MEMORY_N_POOLS := 11;
		  CACHE_POOL_SIZE := 1024;
          DICT_BUF_SIZE := 512;
	   END IF;
	   
	   FAST_POOL_PAGES :=9999;
	   SORT_FLAG = 0;
	   SORT_BLK_SIZE=1;
	   SORT_BUF_SIZE := 10;
	   RLOG_POOL_SIZE := 1024;
	   
	   HJ_BUF_GLOBAL_SIZE := cast(v_mem_mb * 0.0625 as int);
	   HAGR_BUF_GLOBAL_SIZE := cast(v_mem_mb * 0.0625 as int);
	   HJ_BUF_SIZE  :=250;
       HAGR_BUF_SIZE :=250;
	   
	   IF v_mem_mb >= 64000 THEN
	      VM_POOL_TARGET  := 8192;
          SESS_POOL_TARGET  := 8192;
	      FAST_POOL_PAGES :=99999;
	      FAST_ROLL_PAGES :=9999;
	      BUFFER :=BUFFER-3000;
	      CACHE_POOL_SIZE := 2048;
	      RLOG_POOL_SIZE := 2048;
          DICT_BUF_SIZE := 1024;
	      SORT_FLAG = 0;
	      SORT_BLK_SIZE=1;
	      SORT_BUF_SIZE=20; 
	      SORT_BUF_GLOBAL_SIZE= cast(v_mem_mb * 0.04 as int); 
	      
	      HJ_BUF_GLOBAL_SIZE := cast(v_mem_mb * 0.1 as int);
	      HAGR_BUF_GLOBAL_SIZE := cast(v_mem_mb * 0.0625 as int);
	      HJ_BUF_SIZE  :=512;
          HAGR_BUF_SIZE :=512;
          MEMORY_N_POOLS := 59;
          
          IF v_mem_mb >= 128000 OR flag=1 THEN
            SORT_FLAG = 1;
	        SORT_BLK_SIZE=1;
	        SORT_BUF_SIZE=50; 
	        SORT_BUF_GLOBAL_SIZE= cast(v_mem_mb * 0.1 as int); 
          END IF;
          
          IF v_mem_mb >= 256000 THEN
            SORT_FLAG = 1;
	        SORT_BLK_SIZE=2;
	        SORT_BUF_SIZE=50; 
	        SORT_BUF_GLOBAL_SIZE= cast(v_mem_mb * 0.1 as int); 
          END IF;
	   END IF;
	   
       HJ_BUF_GLOBAL_SIZE :=round(HJ_BUF_GLOBAL_SIZE,-3);
       HAGR_BUF_GLOBAL_SIZE :=round(HAGR_BUF_GLOBAL_SIZE,-3);
       SORT_BUF_GLOBAL_SIZE :=round(SORT_BUF_GLOBAL_SIZE,-3);
       RECYCLE :=round(RECYCLE,-3);
	ELSE
	   MEMORY_POOL :=GREAT(cast(v_mem_mb * 0.0625 as int),100);
	   MEMORY_POOL :=round(MEMORY_POOL,-2);
	   MEMORY_N_POOLS := 2;
	   CACHE_POOL_SIZE := 200;
	   RLOG_POOL_SIZE  := 256;
	   SORT_BUF_SIZE := 10;
	   SORT_BUF_GLOBAL_SIZE := 500;
	   DICT_BUF_SIZE := 128;
	   SORT_FLAG = 0;
	   SORT_BLK_SIZE=1;
	   
	   HJ_BUF_GLOBAL_SIZE := round(GREAT(cast(v_mem_mb * 0.0625 as int),500),-2);
	   HAGR_BUF_GLOBAL_SIZE := round(GREAT(cast(v_mem_mb * 0.0625 as int),500),-2);
       HJ_BUF_SIZE := round(GREAT(cast(v_mem_mb * 0.00625 as int),50),-2);
       HAGR_BUF_SIZE :=round(GREAT(cast(v_mem_mb * 0.00625 as int),50),-2);
       
       MAX_SESSIONS  :=100;
       VM_POOL_TARGET  := 8192;
       SESS_POOL_TARGET  := 8192;
	END IF;	
	
	  --设置根据RECYCLE情况RECYCLE_POOLS参数
		with t as
        (
                select rownum rn from dual connect by level <= 100
        ) ,
        t1 as
        (
                select * from t where rn > 1 minus
                select
                        ta.rn * tb.rn
                from
                        t ta,
                        t tb
                where
                        ta.rn <= tb.rn
                    and ta.rn  > 1
                    and tb.rn  > 1
        )
      select top 1 rn into RECYCLE_POOLS from t1 where rn <= great(RECYCLE*1024/3000/(page()/1024),2) order by 1 desc;
	
	
	tname :='BAK_DMINI_' || to_char(sysdate,'yymmdd');
	
	execute IMMEDIATE 'select count(*) from USER_ALL_TABLES where table_name= ?' into CNT using tname;
    if exists(select 1 from V$INSTANCE where MODE$ in ('NORMAL','PRIMARY')) then  
      IF CNT=0 and ini_bak=1 THEN 
	    execute IMMEDIATE 'CREATE TABLE BAK_DMINI_' || to_char(sysdate,'yymmdd') || ' as select *,sysdate uptime from v$dm_ini';
	  ELSE 
	    IF CNT=1 THEN
	      execute IMMEDIATE  'INSERT INTO  BAK_DMINI_' || to_char(sysdate,'yymmdd') || ' select *,sysdate uptime from v$dm_ini';
	      COMMIT;
	    END IF;
	  END IF;
	end if;
	
	--如果oltp_mode设置为1，采用旧的排序模式,undo_relation采用默认值
	if oltp_mode=1 then
	   SORT_FLAG = 0;
	   SORT_BUF_SIZE := 2;
	end if;
	
	--如果oltp_mode设置为0，undo_relation适当放大,采用新的排序方法
	if oltp_mode=0 then
	   UNDO_RETENTION = 900;
	end if;
	
	MAX_BUFFER := BUFFER;
	
	SELECT  
	  TO_NUMBER(SUBSTR(VER,1,2),'XX'),
      TO_NUMBER(SUBSTR(VER,5,2),'XX'),
      TO_NUMBER(SUBSTR(VER,7,2),'XX') into VER0,VER1,VER2
    FROM (SELECT RAWTOHEX(CAST(SUBSTR(VER,3) AS INT)) AS VER
          FROM (SELECT REGEXP_SUBSTR(ID_CODE,'[^-]+',1,1) AS VER));
	
	select round((v_mem_mb-(MEMORY_TARGET+BUFFER+RECYCLE+HJ_BUF_GLOBAL_SIZE+HAGR_BUF_GLOBAL_SIZE+CACHE_POOL_SIZE
	+DICT_BUF_SIZE+SORT_BUF_GLOBAL_SIZE+RLOG_POOL_SIZE))/((VM_POOL_TARGET+SESS_POOL_TARGET)/1024),-2) into MAX_SESSIONS;
	MAX_SESSIONS:=GREAT(MAX_SESSIONS,100);
	
	
	IF exec_mode=0 THEN
		--修改cpu相关参数
		SP_SET_PARA_VALUE(2,'WORKER_THREADS',v_cpus);
		SP_SET_PARA_VALUE(2,'IO_THR_GROUPS',IO_THR_GROUPS);
		--将此参数改为0
		SP_SET_PARA_VALUE(2,'GEN_SQL_MEM_RECLAIM',0);
		
		
		--修改内存池相关参数
		SP_SET_PARA_VALUE(2,'MAX_OS_MEMORY',       100);
		SP_SET_PARA_VALUE(2,'MEMORY_POOL',         MEMORY_POOL);
		SP_SET_PARA_VALUE(2,'MEMORY_N_POOLS',      MEMORY_N_POOLS);
		SP_SET_PARA_VALUE(2,'MEMORY_TARGET',       MEMORY_TARGET);
        --修改内存检测参数为1		
		SP_SET_PARA_VALUE(2,'MEMORY_MAGIC_CHECK',       1);
			
		--修改缓冲区相关参数
		SP_SET_PARA_VALUE(2,'BUFFER',              BUFFER);
		
		--新版本已去掉MAX_BUFFER参数，如果存在就修改
		IF EXISTS (SELECT * FROM V$DM_INI WHERE PARA_NAME='MAX_BUFFER') THEN
		    SP_SET_PARA_VALUE(2,'MAX_BUFFER',          MAX_BUFFER);
		END IF;	  
		SP_SET_PARA_VALUE(2,'BUFFER_POOLS',        BUFFER_POOLS);
		SP_SET_PARA_VALUE(2,'RECYCLE',        	   RECYCLE);
		
			
		SP_SET_PARA_VALUE(2,'RECYCLE_POOLS',       RECYCLE_POOLS);
		
		--修改fast_pool相关参数，如果是dsc环境，适当放小，以免影响启动速度
        IF is_dsc= 1 THEN
           SP_SET_PARA_VALUE(2,'FAST_POOL_PAGES',    10000);	
		   SP_SET_PARA_VALUE(2,'FAST_ROLL_PAGES',     3000);
		   SP_SET_PARA_VALUE(2,'TASK_THREADS',     16);
		   SP_SET_PARA_VALUE(2,'DSC_INSERT_LOCK_ROWS', 0);	   
        ELSE
		   SP_SET_PARA_VALUE(2,'FAST_POOL_PAGES',    FAST_POOL_PAGES);	
		   SP_SET_PARA_VALUE(2,'FAST_ROLL_PAGES',    FAST_ROLL_PAGES);
		   SP_SET_PARA_VALUE(2,'TASK_THREADS',TASK_THREADS);
		   --如果不是dsc环境，开启热页动态加载，关闭预读
		   SP_SET_PARA_VALUE(2,'ENABLE_FREQROOTS',1);
		   SP_SET_PARA_VALUE(2,'MULTI_PAGE_GET_NUM',1);
           SP_SET_PARA_VALUE(2,'PRELOAD_SCAN_NUM',0);
           SP_SET_PARA_VALUE(2,'PRELOAD_EXTENT_NUM',0);
        END IF;
		
		--修改HASH相关参数
		SP_SET_PARA_VALUE(1,'HJ_BUF_GLOBAL_SIZE',  HJ_BUF_GLOBAL_SIZE);
		SP_SET_PARA_VALUE(1,'HJ_BUF_SIZE',         HJ_BUF_SIZE );
		SP_SET_PARA_VALUE(1,'HAGR_BUF_GLOBAL_SIZE',HAGR_BUF_GLOBAL_SIZE);
		SP_SET_PARA_VALUE(1,'HAGR_BUF_SIZE',       HAGR_BUF_SIZE  );
		
		--修改排序相关参数
		SP_SET_PARA_VALUE(2,'SORT_FLAG',SORT_FLAG);
		SP_SET_PARA_VALUE(2,'SORT_BLK_SIZE',SORT_BLK_SIZE);
		SP_SET_PARA_VALUE(2,'SORT_BUF_SIZE',       SORT_BUF_SIZE);
		SP_SET_PARA_VALUE(2,'SORT_BUF_GLOBAL_SIZE',       SORT_BUF_GLOBAL_SIZE);
		
		--修改其他内存参数
		SP_SET_PARA_VALUE(2,'RLOG_POOL_SIZE',      RLOG_POOL_SIZE);
		SP_SET_PARA_VALUE(2,'CACHE_POOL_SIZE',     CACHE_POOL_SIZE);	
		SP_SET_PARA_VALUE(2,'DICT_BUF_SIZE',       DICT_BUF_SIZE); 
		SP_SET_PARA_VALUE(2,'VM_POOL_TARGET',      VM_POOL_TARGET); 
		SP_SET_PARA_VALUE(2,'SESS_POOL_TARGET',    SESS_POOL_TARGET); 
		
		
		--修改实例相关参数
		SP_SET_PARA_VALUE(2,'USE_PLN_POOL',        1); 
		SP_SET_PARA_VALUE(2,'ENABLE_MONITOR',      1); 
		SP_SET_PARA_VALUE(2,'SVR_LOG',             0); 
		SP_SET_PARA_VALUE(2,'TEMP_SIZE',           1024);
		SP_SET_PARA_VALUE(2,'TEMP_SPACE_LIMIT',    102400); 
		SP_SET_PARA_VALUE(2,'MAX_SESSIONS',        MAX_SESSIONS); 
		SP_SET_PARA_VALUE(2,'MAX_SESSION_STATEMENT', 20000); 
		
		--性能要求高且大字段较少的业务场景建议设置为1，大字段多的场景设置为0
		if pk_cluster_mode = 1 then
		  SP_SET_PARA_VALUE(2,'PK_WITH_CLUSTER',1); 
		else
		  SP_SET_PARA_VALUE(2,'PK_WITH_CLUSTER',0);
		end if;
		
		SP_SET_PARA_VALUE(2,'ENABLE_ENCRYPT',0); 
		
		--修改优化器相关参数
		SP_SET_PARA_VALUE(2,'OLAP_FLAG',2); 
		SP_SET_PARA_VALUE(2,'VIEW_PULLUP_FLAG',1);  
		SP_SET_PARA_VALUE(2,'OPTIMIZER_MODE',1); 
		SP_SET_PARA_VALUE(2,'ADAPTIVE_NPLN_FLAG',0); 
		
		--禁用索引监控和位图索引
		SP_SET_PARA_VALUE(2,'MONITOR_INDEX_FLAG',2); 
		SP_SET_PARA_VALUE(2,'ENABLE_CREATE_BM_INDEX_FLAG',0);
		
		
	    IF VER0=8 and VER1>0 AND VER1<=3 AND VER2<=163 THEN
	     SP_SET_PARA_VALUE(2,'OPTIMIZER_OR_NBEXP',0);
	    END IF;
	    
	    --3.175之前的版本BIND_PARAM_OPT_FLAG参数改为0
	    IF VER0=8 and VER1>0 AND VER1<=3 AND VER2<=175 THEN
	     SP_SET_PARA_VALUE(2,'BIND_PARAM_OPT_FLAG',0);
	    END IF;
	
	    IF VER0=8 and VER1>0 AND VER1<=3 AND VER2<=153 THEN
	     IF EXISTS (SELECT * FROM V$DM_INI WHERE PARA_NAME = 'GROUP_OPT_FLAG' and DEFAULT_VALUE=60) THEN
	        SP_SET_PARA_VALUE(2,'GROUP_OPT_FLAG',52);
	     END IF;
	    END IF;
	    
	    IF EXISTS (SELECT * FROM V$DM_INI WHERE PARA_NAME = 'MEM_POOL_EXTEND_MODE') THEN
	        SP_SET_PARA_VALUE(2,'MEM_POOL_EXTEND_MODE',0);
	    END IF;
		
		--8.1.4.189以前的版本OPERATION_NEW_MOTION参数改为0，隐藏参数
	    IF VER0=8 and VER1>0 AND VER1<=4 AND VER2<189 THEN
	        PRINT '-- 8.1.4.189以前的版本OPERATION_NEW_MOTION参数改为0，隐藏参数需要在dm.ini中添加 OPERATION_NEW_MOTION=0';
	    END IF;
		
		-- V8.1.4.111以前的版本HASH_JOIN_LOOP_TIMES参数改为1，隐藏参数
	    IF VER0=8 and VER1>0 AND VER1<=4 AND VER2<111 THEN
	        PRINT '-- V8.1.4.111以前的版本HASH_JOIN_LOOP_TIMES参数改为1，隐藏参数需要在dm.ini中添加 HASH_JOIN_LOOP_TIMES=1';
	    END IF; 
		
		--开启并行PURGE
		SP_SET_PARA_VALUE(2,'PARALLEL_PURGE_FLAG',1);
		--开启手动并行
		SP_SET_PARA_VALUE(2,'PARALLEL_POLICY',2);
		
		SP_SET_PARA_DOUBLE_VALUE(2,'UNDO_RETENTION',UNDO_RETENTION);
		
		--UNDO_RETENTION如果放大，可以适当调大UNDO_EXTENT_NUM。负载高的时候，减少文件系统的申请/释放操作。
		SP_SET_PARA_VALUE(2,'UNDO_EXTENT_NUM',16);
		
		--开启SQL 注入HINT功能
		SP_SET_PARA_VALUE(2,'ENABLE_INJECT_HINT',1);
		
		SP_SET_PARA_VALUE(2,'FAST_LOGIN',1);
		SP_SET_PARA_VALUE(2,'BTR_SPLIT_MODE',1);
		
		--关闭参数监控
		SP_SET_PARA_VALUE(2,'ENABLE_MONITOR_BP',0);
		
		--SLCT_OPT_FLAG参数设置为0
		IF EXISTS (SELECT * FROM V$DM_INI WHERE PARA_NAME='SLCT_OPT_FLAG') THEN
		    SP_SET_PARA_VALUE(1,'SLCT_OPT_FLAG',0);
		  END IF;		
		
		IF is_dsc= 1 THEN 
		   SP_SET_PARA_VALUE(2,'ENABLE_FREQROOTS',0);  
		 --2025Q3 8.1.4.169以前的版本DSC关闭数据页预加载参数，8.1.4.169之后打开
		 IF VER0=8 and VER1>0 AND VER1<=4 AND VER2<169 THEN
           SP_SET_PARA_VALUE(2,'MULTI_PAGE_GET_NUM',1);
           SP_SET_PARA_VALUE(2,'PRELOAD_SCAN_NUM',0);
           SP_SET_PARA_VALUE(2,'PRELOAD_EXTENT_NUM',0);
         ELSE
           SP_SET_PARA_VALUE(2,'MULTI_PAGE_GET_NUM',16);
           SP_SET_PARA_VALUE(2,'PRELOAD_SCAN_NUM',4);
           SP_SET_PARA_VALUE(2,'PRELOAD_EXTENT_NUM',5);
         END IF;
		  
		   SP_SET_PARA_VALUE(2,'DSC_N_POOLS',MEMORY_N_POOLS); 
		  
		  IF EXISTS (SELECT * FROM V$DM_INI WHERE PARA_NAME='DSC_GBS_REVOKE_OPT') THEN
		   SP_SET_PARA_VALUE(2,'DSC_GBS_REVOKE_OPT',0);
		  END IF;
		  
		   SP_SET_PARA_VALUE(2,'DSC_HALT_SYNC',0);
		   SP_SET_PARA_VALUE(2,'DSC_N_CTLS',50000);
           SP_SET_PARA_VALUE(2,'DSC_ENABLE_MONITOR',0);
           SP_SET_PARA_VALUE(2,'TRX_DICT_LOCK_NUM',5);
		   SP_SET_PARA_VALUE(2,'DIRECT_IO',1);
		END IF;
		

	ELSE
		--修改cpu相关参数
		PRINT 'SP_SET_PARA_VALUE(2,''WORKER_THREADS'','||v_cpus||');';
		PRINT 'SP_SET_PARA_VALUE(2,''IO_THR_GROUPS'','||IO_THR_GROUPS||');';
		PRINT 'SP_SET_PARA_VALUE(2,''GEN_SQL_MEM_RECLAIM'',0);';
		
		
		--修改内存池相关参数
		PRINT 'SP_SET_PARA_VALUE(2,''MAX_OS_MEMORY'',       '||100||');';
		PRINT 'SP_SET_PARA_VALUE(2,''MEMORY_POOL'',         '||MEMORY_POOL||');';
		PRINT 'SP_SET_PARA_VALUE(2,''MEMORY_N_POOLS'',      '||MEMORY_N_POOLS||');';
		PRINT 'SP_SET_PARA_VALUE(2,''MEMORY_TARGET'',       '||MEMORY_TARGET||');';	
		
		--修改缓冲区相关参数
		PRINT 'SP_SET_PARA_VALUE(2,''BUFFER'',              '||BUFFER||');';
		
		--新版本已去掉MAX_BUFFER参数，如果存在就修改
		IF EXISTS (SELECT * FROM V$DM_INI WHERE PARA_NAME='MAX_BUFFER') THEN
		   PRINT 'SP_SET_PARA_VALUE(2,''MAX_BUFFER'',          '||MAX_BUFFER||');';
		END IF;	
		PRINT 'SP_SET_PARA_VALUE(2,''BUFFER_POOLS'',        '||BUFFER_POOLS||');';
		PRINT 'SP_SET_PARA_VALUE(2,''RECYCLE'',            '||RECYCLE||');';
		PRINT 'SP_SET_PARA_VALUE(2,''RECYCLE_POOLS'',       '||RECYCLE_POOLS||');';
		
		--修改fast_pool相关参数，如果是dsc环境，适当放小，以免影响启动速度
        IF is_dsc= 1 THEN
           PRINT 'SP_SET_PARA_VALUE(2,''FAST_POOL_PAGES'',  10000);';	
		   PRINT 'SP_SET_PARA_VALUE(2,''FAST_ROLL_PAGES'',   3000);';   
		   PRINT 'SP_SET_PARA_VALUE(2,''TASK_THREADS'',     16);';	
		   PRINT 'SP_SET_PARA_VALUE(2,''DSC_INSERT_LOCK_ROWS'', 0);';
        ELSE
		   PRINT 'SP_SET_PARA_VALUE(2,''FAST_POOL_PAGES'',     '||FAST_POOL_PAGES||');';	
		   PRINT 'SP_SET_PARA_VALUE(2,''FAST_ROLL_PAGES'',     '||FAST_ROLL_PAGES||');'; 
		   --如果不是dsc环境，开启热页动态加载，关闭预读
		   PRINT 'SP_SET_PARA_VALUE(2,''ENABLE_FREQROOTS'',1);';   
		   PRINT 'SP_SET_PARA_VALUE(2,''MULTI_PAGE_GET_NUM'',1);';
           PRINT 'SP_SET_PARA_VALUE(2,''PRELOAD_SCAN_NUM'',0);';
           PRINT 'SP_SET_PARA_VALUE(2,''PRELOAD_EXTENT_NUM'',0);';  
           PRINT 'SP_SET_PARA_VALUE(2,''TASK_THREADS'','||TASK_THREADS||');';                                            
        END IF;
		
		--修改内存检测参数为1		
		PRINT 'SP_SET_PARA_VALUE(2,''MEMORY_MAGIC_CHECK'',       1);';
		
		--修改HASH相关参数
		PRINT 'SP_SET_PARA_VALUE(1,''HJ_BUF_GLOBAL_SIZE'',  '||HJ_BUF_GLOBAL_SIZE||');';
		PRINT 'SP_SET_PARA_VALUE(1,''HJ_BUF_SIZE'',        '||HJ_BUF_SIZE||');';
		PRINT 'SP_SET_PARA_VALUE(1,''HAGR_BUF_GLOBAL_SIZE'','||HAGR_BUF_GLOBAL_SIZE||');';
		PRINT 'SP_SET_PARA_VALUE(1,''HAGR_BUF_SIZE'',     '||HAGR_BUF_SIZE||');';
		
		--修改排序相关参数
		PRINT 'SP_SET_PARA_VALUE(2,''SORT_FLAG'','||SORT_FLAG||');';
		PRINT 'SP_SET_PARA_VALUE(2,''SORT_BLK_SIZE'','||SORT_BLK_SIZE||');';
		PRINT 'SP_SET_PARA_VALUE(2,''SORT_BUF_SIZE'',       '||SORT_BUF_SIZE||');';
		PRINT 'SP_SET_PARA_VALUE(2,''SORT_BUF_GLOBAL_SIZE'',       '||SORT_BUF_GLOBAL_SIZE||');';
		
		--修改其他内存参数
		PRINT 'SP_SET_PARA_VALUE(2,''RLOG_POOL_SIZE'',      '||RLOG_POOL_SIZE||');';
		PRINT 'SP_SET_PARA_VALUE(2,''CACHE_POOL_SIZE'',     '||CACHE_POOL_SIZE||');';	
		PRINT 'SP_SET_PARA_VALUE(2,''DICT_BUF_SIZE'',       '||DICT_BUF_SIZE||');'; 
		PRINT 'SP_SET_PARA_VALUE(2,''VM_POOL_TARGET'',      '||VM_POOL_TARGET||');';
		PRINT 'SP_SET_PARA_VALUE(2,''SESS_POOL_TARGET'',    '||SESS_POOL_TARGET||');';
		
		
		--修改实例相关参数
		PRINT 'SP_SET_PARA_VALUE(2,''USE_PLN_POOL'',        1);';
		PRINT 'SP_SET_PARA_VALUE(2,''ENABLE_MONITOR'',      1);'; 
		PRINT 'SP_SET_PARA_VALUE(2,''SVR_LOG'',             0);'; 
		PRINT 'SP_SET_PARA_VALUE(2,''TEMP_SIZE'',           1024);';
		PRINT 'SP_SET_PARA_VALUE(2,''TEMP_SPACE_LIMIT'',    102400);';
		PRINT 'SP_SET_PARA_VALUE(2,''MAX_SESSIONS'',        '||MAX_SESSIONS||');';
		PRINT 'SP_SET_PARA_VALUE(2,''MAX_SESSION_STATEMENT'', 20000);';
		
		--性能要求高且大字段较少的业务场景建议设置为1，大字段多的场景设置为0
		if pk_cluster_mode = 1 then
		   PRINT 'SP_SET_PARA_VALUE(2,''PK_WITH_CLUSTER'',		1);';
		else
		   PRINT 'SP_SET_PARA_VALUE(2,''PK_WITH_CLUSTER'',		0);';
		end if;
		
		PRINT 'SP_SET_PARA_VALUE(2,''ENABLE_ENCRYPT'',0);';
		
		--修改优化器相关参数
		PRINT 'SP_SET_PARA_VALUE(2,''OLAP_FLAG'',2);';
		PRINT 'SP_SET_PARA_VALUE(2,''VIEW_PULLUP_FLAG'',1);';
		PRINT 'SP_SET_PARA_VALUE(2,''OPTIMIZER_MODE'',1);';
		PRINT 'SP_SET_PARA_VALUE(2,''ADAPTIVE_NPLN_FLAG'',0);';
		
		--禁用索引监控和位图索引
		PRINT 'SP_SET_PARA_VALUE(2,''MONITOR_INDEX_FLAG'',2);';
		PRINT 'SP_SET_PARA_VALUE(2,''ENABLE_CREATE_BM_INDEX_FLAG'',0);';
		
		
		--3.163 之前的版本OPTIMIZER_OR_NBEXP不能包含16
		IF VER0=8 and VER1>0 AND VER1<=3 AND VER2<163 THEN
	     PRINT 'SP_SET_PARA_VALUE(2,''OPTIMIZER_OR_NBEXP'',0);';
	    END IF;
	    
	    --3.175之前的版本BIND_PARAM_OPT_FLAG参数改为0
	    IF VER0=8 and VER1>0 AND VER1<=3 AND VER2<=175 THEN
	     PRINT 'SP_SET_PARA_VALUE(2,''BIND_PARAM_OPT_FLAG'',0);';
	    END IF;
	   
	   --3.153 之前的版本GROUP_OPT_FLAG不能包含8
	   IF VER0=8 and VER1>0 AND VER1<=3 AND VER2<153 THEN
	     IF EXISTS (SELECT * FROM V$DM_INI WHERE PARA_NAME = 'GROUP_OPT_FLAG' and DEFAULT_VALUE=60) THEN
	        PRINT 'SP_SET_PARA_VALUE(2,''GROUP_OPT_FLAG'',52);';
	     END IF;
	    END IF;
	    
	   IF EXISTS (SELECT * FROM V$DM_INI WHERE PARA_NAME = 'MEM_POOL_EXTEND_MODE') THEN
	        PRINT 'SP_SET_PARA_VALUE(2,''MEM_POOL_EXTEND_MODE'',0);';
	   END IF;
	   
		
		--开启并行PURGE
		PRINT 'SP_SET_PARA_VALUE(2,''PARALLEL_PURGE_FLAG'',1);';
		--开启手动并行
		PRINT 'SP_SET_PARA_VALUE(2,''PARALLEL_POLICY'',2);';
		
		PRINT 'SP_SET_PARA_DOUBLE_VALUE(2,''UNDO_RETENTION'','||UNDO_RETENTION||');';
		
		--UNDO_RETENTION如果放大，可以适当调大UNDO_EXTENT_NUM。负载高的时候，减少文件系统的申请/释放操作。
		PRINT 'SP_SET_PARA_VALUE(2,''UNDO_EXTENT_NUM'',16);';
		
		--开启INJECT HINT功能
		PRINT 'SP_SET_PARA_VALUE(2,''ENABLE_INJECT_HINT'',1);';

		PRINT 'SP_SET_PARA_VALUE(2,''BTR_SPLIT_MODE'',1);';
        PRINT 'SP_SET_PARA_VALUE(2,''FAST_LOGIN'',1);';
        
        --关闭参数监控
        PRINT 'SP_SET_PARA_VALUE(2,''ENABLE_MONITOR_BP'',0);';
        
        --SLCT_OPT_FLAG参数设置为0
		IF EXISTS (SELECT * FROM V$DM_INI WHERE PARA_NAME='SLCT_OPT_FLAG') THEN
		    PRINT 'SP_SET_PARA_VALUE(1,''SLCT_OPT_FLAG'',0);';
		  END IF;
		 
	      	
	   IF is_dsc= 1 THEN      
	     PRINT 'SP_SET_PARA_VALUE(2,''ENABLE_FREQROOTS'',0);';
	     --2025Q3 8.1.4.169以前的版本DSC关闭数据页预加载参数，8.1.4.169之后打开
	     IF VER0=8 and VER1>0 AND VER1<=4 AND VER2<169 THEN
           PRINT 'SP_SET_PARA_VALUE(2,''MULTI_PAGE_GET_NUM'',1);';
           PRINT 'SP_SET_PARA_VALUE(2,''PRELOAD_SCAN_NUM'',0);';
           PRINT 'SP_SET_PARA_VALUE(2,''PRELOAD_EXTENT_NUM'',0);';
         ELSE
           PRINT 'SP_SET_PARA_VALUE(2,''MULTI_PAGE_GET_NUM'',16);';
           PRINT 'SP_SET_PARA_VALUE(2,''PRELOAD_SCAN_NUM'',4);';
           PRINT 'SP_SET_PARA_VALUE(2,''PRELOAD_EXTENT_NUM'',5);';
         END IF;
         
	  
		  PRINT 'SP_SET_PARA_VALUE(2,''DSC_N_POOLS'',' ||MEMORY_N_POOLS ||');'; 
		  IF EXISTS (SELECT * FROM V$DM_INI WHERE PARA_NAME='DSC_GBS_REVOKE_OPT') THEN
		    PRINT 'SP_SET_PARA_VALUE(2,''DSC_GBS_REVOKE_OPT'',0);';
		  END IF;
		  PRINT 'SP_SET_PARA_VALUE(2,''DSC_HALT_SYNC'',0);';
		  PRINT 'SP_SET_PARA_VALUE(2,''DSC_N_CTLS'',50000);';
          PRINT 'SP_SET_PARA_VALUE(2,''DSC_ENABLE_MONITOR'',0);';
          PRINT 'SP_SET_PARA_VALUE(2,''TRX_DICT_LOCK_NUM'',5);';
		  PRINT 'SP_SET_PARA_VALUE(2,''DIRECT_IO'',1);';
		  
		  --8.1.4.189以前的版本OPERATION_NEW_MOTION参数改为0，隐藏参数
	      IF VER0=8 and VER1>0 AND VER1<=4 AND VER2<189 THEN
	        PRINT '-- 8.1.4.189以前的版本OPERATION_NEW_MOTION参数改为0，隐藏参数需要在dm.ini中添加 OPERATION_NEW_MOTION=0';
	      END IF;
	      
	      -- V8.1.4.111以前的版本HASH_JOIN_LOOP_TIMES参数改为1，隐藏参数
	      IF VER0=8 and VER1>0 AND VER1<=4 AND VER2<111 THEN
	        PRINT '-- V8.1.4.111以前的版本HASH_JOIN_LOOP_TIMES参数改为1，隐藏参数需要在dm.ini中添加 HASH_JOIN_LOOP_TIMES=1';
	      END IF; 
	     
	   END IF;
		
		
	END IF;
	
	
	select MEMORY_TARGET+BUFFER+RECYCLE+HJ_BUF_GLOBAL_SIZE+HAGR_BUF_GLOBAL_SIZE+CACHE_POOL_SIZE
	+DICT_BUF_SIZE+SORT_BUF_GLOBAL_SIZE+RLOG_POOL_SIZE+MAX_SESSIONS*((VM_POOL_TARGET+SESS_POOL_TARGET)/1024);
	
		
	exception
      when others then
         raise_application_error (-20001,substr( ' 执行失败, '||SQLCODE||' '||SQLERRM||' '||dbms_utility.format_error_backtrace  , 1, 400));
	
	<<return_2000>> null;
end;
/

